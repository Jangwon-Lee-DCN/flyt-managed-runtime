use std::{fs, io::BufReader, os::unix::net::{UnixListener, UnixStream}, path::Path};

use crate::{bookkeeping::{VMResources, VMResourcesGetter}, client_handler::FlytClientManager, common::{api_commands::FrontEndCommand, utils::StreamUtils}, servernode_handler::ServerNodesManager};


#[derive(PartialEq, Debug)]
enum ChangeConfigFor {
    SmCores,
    Memory,
    Both,
}

pub struct FrontendHandler<'a> {
    client_mgr: &'a FlytClientManager<'a>,
    server_nodes_manager: &'a ServerNodesManager<'a>,
    resources: &'a VMResourcesGetter,
}

impl <'a> FrontendHandler<'a> {
    pub fn new(client_mgr: &'a FlytClientManager, server_nodes_manager: &'a ServerNodesManager, resources: &'a VMResourcesGetter) -> Self {
        FrontendHandler {
            client_mgr,
            server_nodes_manager,
            resources,
        }
    }

    pub fn start_listening(&self, socket_path: &str) {

        if Path::new(socket_path).exists() {
            fs::remove_file(socket_path).unwrap();
        }

        let listener = UnixListener::bind(socket_path).unwrap();

        log::info!("Frontend handler listening on {}", socket_path);

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    self.handle_request(stream);
                }
                Err(e) => {
                    log::error!("Error: {}", e);
                    break;
                }
            }
        }
    }

    fn handle_request(&self, mut stream: UnixStream) {
        let reader_clone = match stream.try_clone() {
            Ok(stream) => stream,
            Err(e) => {
                log::error!("Error cloning stream: {}", e);
                return;
            }
        };
        let mut reader = BufReader::new(reader_clone);
        
        let command = match StreamUtils::read_line(&mut reader) {
            Ok(command) => command,
            Err(e) => {
                log::error!("Error reading command: {}", e);
                return;
            }
        };
        
        match command.as_str() {
            FrontEndCommand::LIST_VMS => {
                self.list_vms(stream);
            }
            FrontEndCommand::LIST_SERVER_NODES => {
                self.list_servernodes(stream);
            }
            FrontEndCommand::LIST_VIRT_SERVERS => {
                self.list_virt_servers(stream);
            }
            FrontEndCommand::CHANGE_SM_CORES => {
                self.change_resources(stream, reader, ChangeConfigFor::SmCores);
            }
            FrontEndCommand::CHANGE_MEMORY => {
                self.change_resources(stream, reader, ChangeConfigFor::Memory);
            }
            FrontEndCommand::CHANGE_SM_CORES_AND_MEMORY => {
                self.change_resources(stream, reader, ChangeConfigFor::Both);
            }
            FrontEndCommand::MIGRATE_VIRT_SERVER => {
                self.migrate_vm(stream, reader);
            }
            FrontEndCommand::MIGRATE_VIRT_SERVER_AUTO => {
                self.migrate_vm_auto(stream, reader);
            }
            FrontEndCommand::UPSERT_SESSION => self.upsert_session(stream, reader),
            FrontEndCommand::GET_SESSION => self.get_session(stream, reader),
            FrontEndCommand::DELETE_SESSION => self.delete_session(stream, reader),
            FrontEndCommand::GET_CAPABILITIES => {
                let _ = StreamUtils::write_all(
                    &mut stream,
                    "200\nmanaged-session-v1,whole-gpu-mps,mig\n".to_string(),
                );
            }
            _ => {
                log::error!("Invalid command: {}", command);
            }
        }
    }

    fn upsert_session(&self, mut stream: UnixStream, mut reader: BufReader<UnixStream>) {
        let values = match StreamUtils::read_response(&mut reader, 1) {
            Ok(lines) => lines[0].split(',').map(str::to_string).collect::<Vec<_>>(),
            Err(error) => {
                let _ = StreamUtils::write_all(&mut stream, format!("400\n{}\n", error));
                return;
            }
        };
        if values.len() != 10 {
            let _ = StreamUtils::write_all(&mut stream, "400\nExpected 10 fields\n".to_string());
            return;
        }
        let resources = VMResources {
            workload_id: Some(values[0].clone()), tenant_id: Some(values[1].clone()),
            attachment_id: Some(values[2].clone()), client_address: values[3].clone(), preferred_node: values[4].clone(),
            profile: Some(values[5].clone()),
            compute_units: match values[6].parse() { Ok(value) => value, Err(_) => { let _ = StreamUtils::write_all(&mut stream, "400\nInvalid compute units\n".to_string()); return; } },
            memory: match values[7].parse() { Ok(value) => value, Err(_) => { let _ = StreamUtils::write_all(&mut stream, "400\nInvalid memory\n".to_string()); return; } },
            generation: match values[8].parse() { Ok(value) => Some(value), Err(_) => { let _ = StreamUtils::write_all(&mut stream, "400\nInvalid generation\n".to_string()); return; } },
            state: Some(if self.server_nodes_manager.get_all_server_nodes().is_empty() { "PENDING_CAPACITY" } else { "SESSION_CREATED" }.to_string()),
            credential_hash: Some(crate::bookkeeping::credential_hash(&values[9])),
        };
        match self.resources.upsert_session(&resources) {
            Ok(()) => { let _ = StreamUtils::write_all(&mut stream, format!("200\n{}\n", resources.state.unwrap())); },
            Err(error) => { let _ = StreamUtils::write_all(&mut stream, format!("500\n{}\n", error)); },
        }
    }

    fn get_session(&self, mut stream: UnixStream, mut reader: BufReader<UnixStream>) {
        let uuid = match StreamUtils::read_line(&mut reader) { Ok(value) => value, Err(error) => { let _ = StreamUtils::write_all(&mut stream, format!("400\n{}\n", error)); return; } };
        match self.resources.get_session(&uuid) {
            Ok(Some(value)) => { let _ = StreamUtils::write_all(&mut stream, format!("200\n{},{},{},{}\n", value.workload_id.unwrap_or_default(), value.client_address, value.generation.unwrap_or_default(), value.state.unwrap_or_default())); },
            Ok(None) => { let _ = StreamUtils::write_all(&mut stream, "404\nSession not found\n".to_string()); },
            Err(error) => { let _ = StreamUtils::write_all(&mut stream, format!("500\n{}\n", error)); },
        }
    }

    fn delete_session(&self, mut stream: UnixStream, mut reader: BufReader<UnixStream>) {
        let uuid = match StreamUtils::read_line(&mut reader) { Ok(value) => value, Err(error) => { let _ = StreamUtils::write_all(&mut stream, format!("400\n{}\n", error)); return; } };
        let session = match self.resources.get_session(&uuid) {
            Ok(value) => value,
            Err(error) => { let _ = StreamUtils::write_all(&mut stream, format!("500\n{}\n", error)); return; },
        };
        if let Some(value) = session {
            if let Err(error) = self.client_mgr.deallocate_and_remove(&value.client_address) {
                let _ = StreamUtils::write_all(&mut stream, format!("409\n{}\n", error));
                return;
            }
        }
        match self.resources.delete_session(&uuid) {
            Ok(()) => { let _ = StreamUtils::write_all(&mut stream, "200\nDELETED\n".to_string()); },
            Err(error) => { let _ = StreamUtils::write_all(&mut stream, format!("500\n{}\n", error)); },
        }
    }

    fn migrate_vm(&self, mut stream: UnixStream, mut reader: BufReader<UnixStream>) {

        log::info!("Received migrate request");

        let request_params = match StreamUtils::read_response(&mut reader, 1) {
            Ok(params) => params,
            Err(e) => {
                log::error!("Error reading request params: {}", e);
                return;
            }
        };
        
        let parts: Vec<&str> = request_params[0].split(',').collect();
        if parts.len() != 5 {
            log::error!("Invalid arguments for migrate command: {:?}", parts);
            let _ = StreamUtils::write_all(&mut stream, "400\nInvalid arguments\n".to_string());
            return;
        }

        let ipaddr = parts[0];
        let new_server_ip = parts[1];
        let new_server_gpu_id = match parts[2].parse::<u64>() {
            Ok(gpu_id) => gpu_id,
            Err(e) => {
                log::error!("Error parsing gpu_id: {}", e);
                let _ = StreamUtils::write_all(&mut stream, "400\nInvalid arguments\n".to_string());
                return;
            }
        };

        let new_server_compute_units = match parts[3].parse::<u32>() {
            Ok(sm_cores) => sm_cores,
            Err(e) => {
                log::error!("Error parsing sm_cores: {}", e);
                let _ = StreamUtils::write_all(&mut stream, "400\nInvalid arguments\n".to_string());
                return;
            }
        };

        let new_server_memory = match parts[4].parse::<u64>() {
            Ok(memory) => memory,
            Err(e) => {
                log::error!("Error parsing memory: {}", e);
                let _ = StreamUtils::write_all(&mut stream, "400\nInvalid arguments\n".to_string());
                return;
            }
        };

        log::info!("Migrating VM: {} to server: {} with gpu_id: {}", ipaddr, new_server_ip, new_server_gpu_id);
        let res = self.server_nodes_manager.migrate_virt_server(
            self.client_mgr, 
            &ipaddr.to_string(),
            &new_server_ip.to_string(),
            new_server_gpu_id,
            new_server_compute_units,
            new_server_memory);
        
        if res.is_err() {
            log::error!("Error migrating VM: {}", res.clone().unwrap_err());
            
            // resume the client if stopped
            let _ = self.client_mgr.resume_client(ipaddr);

            let _ = StreamUtils::write_all(&mut stream, format!("500\n{}\n", res.unwrap_err()));
        }
        else {
            log::info!("VM migrated successfully");
            let _ = StreamUtils::write_all(&mut stream, "200\nVM migrated successfully\n".to_string());
        }
    
    }

    fn migrate_vm_auto(&self, mut stream: UnixStream, mut reader: BufReader<UnixStream>) {

        log::info!("Received migrate auto request");

        let request_params = match StreamUtils::read_response(&mut reader, 1) {
            Ok(params) => params,
            Err(e) => {
                log::error!("Error reading request params: {}", e);
                return;
            }
        };
        
        let parts: Vec<&str> = request_params[0].split(',').collect();
        if parts.len() != 3 {
            log::error!("Invalid arguments for migrate command: {:?}", parts);
            let _ = StreamUtils::write_all(&mut stream, "400\nInvalid arguments\n".to_string());
            return;
        }

        let ipaddr = parts[0];

        let new_server_compute_units = match parts[1].parse::<u32>() {
            Ok(sm_cores) => sm_cores,
            Err(e) => {
                log::error!("Error parsing sm_cores: {}", e);
                let _ = StreamUtils::write_all(&mut stream, "400\nInvalid arguments\n".to_string());
                return;
            }
        };

        let new_server_memory = match parts[2].parse::<u64>() {
            Ok(memory) => memory,
            Err(e) => {
                log::error!("Error parsing memory: {}", e);
                let _ = StreamUtils::write_all(&mut stream, "400\nInvalid arguments\n".to_string());
                return;
            }
        };

        log::info!("Migrating VM: {}", ipaddr);
        let res = self.server_nodes_manager.migrate_virt_server_auto(
            self.client_mgr, 
            &ipaddr.to_string(),
            new_server_compute_units,
            new_server_memory);
        
        if res.is_err() {
            log::error!("Error migrating VM: {}", res.clone().unwrap_err());
            
            // resume the client if stopped
            let _ = self.client_mgr.resume_client(ipaddr);

            let _ = StreamUtils::write_all(&mut stream, format!("500\n{}\n", res.unwrap_err()));
        }
        else {
            log::info!("VM migrated successfully to server: {}", res.unwrap().read().unwrap().ipaddr );
            let _ = StreamUtils::write_all(&mut stream, "200\nVM migrated successfully\n".to_string());
        }
    
    }

    fn list_vms(&self, mut stream: UnixStream){
        let vms = self.client_mgr.get_all_clients();
        let mut response = String::new();
        response.push_str(format!("200\n{}\n", vms.len()).as_str());
        for vm in vms {
            // format: vmip,servnode_ip,servnode_rpcid,sm_cores,memory,isactive
            if let Some(virt_server) = vm.virt_server {
                let virt_server = virt_server.read().unwrap();
                response.push_str(&format!("{},{},{},{},{},{}\n",
                    vm.ipaddr,
                    virt_server.ipaddr,
                    virt_server.rpc_id,
                    virt_server.compute_units,
                    virt_server.memory,
                    *vm.is_active.read().unwrap()
                ));
            }
            else {
                response.push_str(&format!("{},,,,,{}\n",
                    vm.ipaddr,
                    *vm.is_active.read().unwrap()
                ));
            }
            
        }
        let _ = StreamUtils::write_all(&mut stream, response);
    }

    fn list_servernodes(&self, mut stream: UnixStream) {
        let server_nodes = self.server_nodes_manager.get_all_server_nodes();
        let mut response = String::new();
        response.push_str(format!("200\n{}\n", server_nodes.len()).as_str());
        for server_node in server_nodes {
            // format: ipaddr,num_vgpus
            response.push_str(&format!("{},{}\n", server_node.ipaddr, server_node.gpus.len()));

            for gpu in server_node.gpus.iter() {
                // format: gpuid,name,memory,allocated_memory,compute_units,allocated_compute_units
                let gpu = gpu.read().unwrap();
                response.push_str(&format!("{},{},{},{},{},{}\n",
                    gpu.gpu_id,
                    gpu.name,
                    gpu.memory,
                    gpu.allocated_memory,
                    gpu.compute_units,
                    gpu.allocated_compute_units
                ));
            }
        }
        let _ = StreamUtils::write_all(&mut stream, response);
    }

    fn list_virt_servers(&self, mut stream: UnixStream) {
        let mut response = String::new();
        let serv_nodes = self.server_nodes_manager.get_all_server_nodes();
        for serv_node in serv_nodes {
            for virt_server in serv_node.virt_servers.iter() {
                // format: ipaddr,rpc_id,gpu_id,compute_units,memory
                let virt_server = virt_server.read().unwrap();
                response.push_str(&format!("{},{},{},{},{}\n",
                    virt_server.ipaddr,
                    virt_server.rpc_id,
                    virt_server.gpu.read().unwrap().gpu_id,
                    virt_server.compute_units,
                    virt_server.memory,
                ));
            }
        }
        response.insert_str(0, format!("200\n{}\n", response.lines().count()).as_str());
        let _ = StreamUtils::write_all(&mut stream, response);
    }

    fn change_resources(&self, mut stream: UnixStream, mut reader: BufReader<UnixStream>, change_for: ChangeConfigFor) {

        log::info!("Received change resource request");

        let buffer = match StreamUtils::read_response(&mut reader, 1) {
            Ok(buffer) => buffer,
            Err(e) => {
                log::error!("Error reading buffer: {}", e);
                return;
            }
        };
        let parts: Vec<&str> = buffer[0].split(',').collect();
        if (change_for != ChangeConfigFor::Both && parts.len() != 2) || (change_for == ChangeConfigFor::Both && parts.len() != 3 ){
            log::error!("Invalid arguments for change resource command {:?} {:?}", change_for, parts);
            let _ = StreamUtils::write_all(&mut stream, "400\nInvalid arguments\n".to_string());
            return;
        }
        let ipaddr = parts[0];
        let new_resource = parts[1].parse::<u64>().unwrap();

        log::info!("Changing resource for VM: {}, new resource: {} for {:?}", ipaddr, new_resource, change_for);

        let client = self.client_mgr.get_client(ipaddr);
        if client.is_none() {
            log::error!("Client VM {} not found", ipaddr);
            let _ = StreamUtils::write_all(&mut stream, "500\nVM not found\n".to_string());
            return;
        }

        let client = client.unwrap();
        
        if client.virt_server.is_none() {
            log::error!("VM {} is not running on any server node", ipaddr);
            let _ = StreamUtils::write_all(&mut stream, "500\nVM is not running on any server node\n".to_string());
            return;
        }

        let (virt_server_ip, virt_server_rpc_id, cur_compute, cur_mem) = {
            let virt_server = client.virt_server.as_ref().unwrap().read().unwrap();
            (virt_server.ipaddr.clone(), virt_server.rpc_id, virt_server.compute_units, virt_server.memory)
        };

        let ret = match change_for {
            ChangeConfigFor::SmCores => self.server_nodes_manager.change_resource_configurations(&virt_server_ip, virt_server_rpc_id, new_resource as u32, cur_mem),
            ChangeConfigFor::Memory => self.server_nodes_manager.change_resource_configurations(&virt_server_ip, virt_server_rpc_id, cur_compute, new_resource),
            ChangeConfigFor::Both => {
                let mem_new = parts[2].parse::<u64>().unwrap();
                self.server_nodes_manager.change_resource_configurations(&virt_server_ip, virt_server_rpc_id, new_resource as u32, mem_new)
            }
        };

        if ret.is_ok() {
            log::info!("Resource updated successfully");
            let _ = StreamUtils::write_all(&mut stream, "200\nResource updated successfully\n".to_string());
        }
        else {
            log::error!("Error updating resource: {:?}", ret);
            let _ = StreamUtils::write_all(&mut stream, format!("500\n{}\n", ret.unwrap_err()));
        }

    }
    
}
