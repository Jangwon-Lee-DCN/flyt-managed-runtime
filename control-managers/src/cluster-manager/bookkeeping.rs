use std::sync::RwLock;
use std::sync::Arc;
use std::net::TcpStream;
use toml::Table;
use mongodb::{options::{ClientOptions, ServerAddress, Credential, ReplaceOptions}, sync::Client, sync::Collection, bson::doc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::common::config::cluster_manager_config_path;
use crate::common::types::StreamEnds;
use crate::common::utils::Utils;

static VIRT_SERVER_DEALLOCATE_TIME: RwLock<Option<Option<u64>>> = RwLock::new(None);

#[derive(Debug, Clone)]
pub struct GPU {
    pub name: String,
    pub memory: u64,
    pub compute_units: u32,
    pub compute_power: u64,
    pub gpu_id: u64,
    pub allocated_memory: u64,
    pub allocated_compute_units: u32,
}

impl Default for GPU {
    fn default() -> Self {
        GPU {
            name: "".to_string(),
            memory: 0,
            compute_units: 0,
            compute_power: 0,
            gpu_id: 0,
            allocated_memory: 0,
            allocated_compute_units: 0,
        }
    }
}

#[derive(Debug)]
pub struct ServerNode {
    pub ipaddr: String,
    pub gpus: Vec<Arc<RwLock<GPU>>>,
    pub stream: Arc<RwLock<StreamEnds<TcpStream>>>,
    pub virt_servers: Vec<Arc<RwLock<VirtServer>>>,
}

impl Clone for ServerNode {
    fn clone(&self) -> Self {
        ServerNode {
            ipaddr: self.ipaddr.clone(),
            gpus: self.gpus.clone(),
            stream: self.stream.clone(),
            virt_servers: self.virt_servers.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub struct VirtServer {
    pub ipaddr: String,
    pub compute_units: u32,
    pub memory: u64,
    pub rpc_id: u64,
    pub gpu: Arc<RwLock<GPU>>,
}



#[derive(Debug, Serialize, Deserialize)]
pub struct VMResources {
    #[serde(default, alias = "instance_uuid")]
    pub workload_id: Option<String>,
    #[serde(default, alias = "project_id")]
    pub tenant_id: Option<String>,
    #[serde(default, alias = "port_id")]
    pub attachment_id: Option<String>,
    #[serde(alias = "vm_ip")]
    pub client_address: String,
    #[serde(alias = "host_ip")]
    pub preferred_node: String,
    pub compute_units: u32,
    pub memory: u64,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub generation: Option<u64>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub credential_hash: Option<String>,
}

pub struct VMResourcesGetter {
    mongo_collection: Option<Collection<VMResources>>,
}



impl VMResourcesGetter {

    pub fn new() -> Self {
        let config: Table = Utils::load_config_file(&cluster_manager_config_path());

        let get_collection = || -> Option<Collection<VMResources>> {

            let db_details = config.get("vm-resource-db")?.as_table()?;
            let db_host = db_details.get("host")?.as_str()?;
            let db_port = db_details.get("port")?.as_integer()?;
            let db_user = db_details.get("user")?.as_str()?;
            let db_password = db_details.get("password")?.as_str()?;
            let db_dbname = db_details.get("dbname")?.as_str()?;
            let db_auth_source = db_details.get("auth-source")
                .and_then(|value| value.as_str()).unwrap_or("admin");

            let client_options = ClientOptions::builder()
            .hosts(vec![ServerAddress::Tcp {
                host: db_host.to_string(),
                port: Some(db_port as u16),
            }])
            .credential(Credential::builder()
                .username(db_user.to_string())
                .password(db_password.to_string())
                .source(db_auth_source.to_string())
                .build())
            .build();
            
            let client = Client::with_options(client_options).ok()?;

            Some(client.database(db_dbname).collection("vm_required_resources"))
        };

        Self {
            mongo_collection: get_collection(),
        }

    }

    pub fn get_vm_required_resources(&self, client_address: &String) -> Option<VMResources> {
        // let mut lock = self.mongo_client.try_lock().ok()?;
        let collection = self.mongo_collection.as_ref()?;
        let filter = doc! { "$or": [
            { "client_address": client_address },
            { "vm_ip": client_address },
        ] };
        collection.find_one(filter, None).ok()?.and_then(|mut rsc| {
            rsc.memory = rsc.memory * 1024 * 1024;
            Some(rsc)
        })
    }

    pub fn upsert_session(&self, resources: &VMResources) -> Result<(), String> {
        let collection = self.mongo_collection.as_ref().ok_or("MongoDB is not configured")?;
        let workload_id = resources.workload_id.as_ref().ok_or("workload ID is required")?;
        collection.replace_one(
            doc! { "$or": [
                { "workload_id": workload_id },
                { "instance_uuid": workload_id },
            ] },
            resources,
            ReplaceOptions::builder().upsert(true).build(),
        ).map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn get_session(&self, workload_id: &str) -> Result<Option<VMResources>, String> {
        let collection = self.mongo_collection.as_ref().ok_or("MongoDB is not configured")?;
        collection.find_one(doc! { "$or": [
            { "workload_id": workload_id },
            { "instance_uuid": workload_id },
        ] }, None)
            .map_err(|error| error.to_string())
    }

    pub fn delete_session(&self, workload_id: &str) -> Result<(), String> {
        let collection = self.mongo_collection.as_ref().ok_or("MongoDB is not configured")?;
        collection.delete_one(doc! { "$or": [
            { "workload_id": workload_id },
            { "instance_uuid": workload_id },
        ] }, None)
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn validate_client(&self, workload_id: &str, generation: u64,
                           credential: &str, _observed_ip: &str) -> Result<(), String> {
        let session = self.get_session(workload_id)?
            .ok_or("Session not found")?;
        // The observed peer may be a rack-local TCP relay, NAT gateway, or
        // service mesh sidecar.  It is a transport attribute, not workload
        // identity.  Bind authentication to the managed workload generation
        // and its random credential; retain client_address for routing and
        // attachment reconciliation only.
        if session.generation != Some(generation) {
            return Err("Session identity mismatch".to_string());
        }
        let supplied = credential_hash(credential);
        if session.credential_hash.as_deref() != Some(supplied.as_str()) {
            return Err("Session credential mismatch".to_string());
        }
        Ok(())
    }


}

pub fn credential_hash(credential: &str) -> String {
    format!("{:x}", Sha256::digest(credential.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{credential_hash, VMResources};
    use mongodb::bson::{doc, from_document, to_document};

    #[test]
    fn credential_hash_is_deterministic_and_not_plaintext() {
        let value = credential_hash("bootstrap-secret");
        assert_eq!(64, value.len());
        assert_eq!(value, credential_hash("bootstrap-secret"));
        assert_ne!(value, credential_hash("another-secret"));
        assert!(!value.contains("bootstrap-secret"));
    }

    #[test]
    fn managed_session_serializes_platform_neutral_fields() {
        let session = VMResources {
            workload_id: Some("workload-1".to_string()),
            tenant_id: Some("tenant-1".to_string()),
            attachment_id: Some("attachment-1".to_string()),
            client_address: "192.0.2.10".to_string(),
            preferred_node: "".to_string(),
            compute_units: 8,
            memory: 4096,
            profile: Some("gpu-small".to_string()),
            generation: Some(1),
            state: Some("PENDING_CAPACITY".to_string()),
            credential_hash: Some(credential_hash("bootstrap-secret")),
        };

        let value = to_document(&session).unwrap();
        assert_eq!(Some("workload-1"), value.get_str("workload_id").ok());
        assert!(value.get("instance_uuid").is_none());
        assert!(value.get("project_id").is_none());
        assert!(value.get("port_id").is_none());
        assert!(value.get("vm_ip").is_none());
    }

    #[test]
    fn managed_session_reads_legacy_openstack_fields() {
        let session: VMResources = from_document(doc! {
            "instance_uuid": "legacy-instance",
            "project_id": "legacy-project",
            "port_id": "legacy-port",
            "vm_ip": "192.0.2.11",
            "host_ip": "",
            "compute_units": 8,
            "memory": 4096_i64,
        }).unwrap();

        assert_eq!(Some("legacy-instance"), session.workload_id.as_deref());
        assert_eq!(Some("legacy-project"), session.tenant_id.as_deref());
        assert_eq!(Some("legacy-port"), session.attachment_id.as_deref());
        assert_eq!("192.0.2.11", session.client_address);
    }
}

pub fn get_ckp_base_path() -> Option<String> {
    let config: Table = Utils::load_config_file(&cluster_manager_config_path());
    config.get("migration")?.get("ckp-path")?.as_str().map(|s| s.to_string())
}

pub fn get_virt_server_deallocate_time() -> Option<u64> {

    if let Some(deallocate_time) = VIRT_SERVER_DEALLOCATE_TIME.read().unwrap().clone() {
        return deallocate_time;
    }

    let config: Table = Utils::load_config_file(&cluster_manager_config_path());

    let enabled = config.get("virt-server-auto-deallocate")?.get("enabled")?.as_bool()?;
    if !enabled {
        return None;
    }
    let deallocate_time = config.get("virt-server-auto-deallocate")?.get("grace-period")?.as_integer()?;
    let deallocate_time = Some(deallocate_time as u64);

    VIRT_SERVER_DEALLOCATE_TIME.write().unwrap().replace(deallocate_time);
    deallocate_time
}

pub fn get_ports() -> (u16, u16) {
    let config: Table = Utils::load_config_file(&cluster_manager_config_path());
    let node_port = config.get("ports").unwrap().get("node").unwrap().as_integer().unwrap() as u16;
    let client_port = config.get("ports").unwrap().get("client").unwrap().as_integer().unwrap() as u16;
    (node_port, client_port)
}
