
pub const CLMGR_CONFIG_PATH: &str = "/home/vm/configs/client-mgr.toml";
pub const RMGR_CONFIG_PATH: &str = "/home/ub-12-3/configs/cluster-mgr-config.toml";
pub const SNODE_CONFIG_PATH: &str = "/home/ub-12-3/configs/servnode-config.toml";

pub fn cluster_manager_config_path() -> String {
    std::env::var("FLYT_CLUSTER_MANAGER_CONFIG")
        .unwrap_or_else(|_| RMGR_CONFIG_PATH.to_string())
}

pub fn client_manager_config_path() -> String {
    std::env::var("FLYT_CLIENT_MANAGER_CONFIG")
        .unwrap_or_else(|_| CLMGR_CONFIG_PATH.to_string())
}

pub fn node_manager_config_path() -> String {
    std::env::var("FLYT_NODE_MANAGER_CONFIG")
        .unwrap_or_else(|_| SNODE_CONFIG_PATH.to_string())
}
