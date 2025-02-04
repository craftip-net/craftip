use shared::addressing::DistributorError;
use shared::socket_packet::ClientToProxy;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

type ServerHostname = String;
pub type Tx = mpsc::UnboundedSender<ClientToProxy>;
pub type Rx = mpsc::UnboundedReceiver<ClientToProxy>;

#[derive(Debug, Default, Clone)]
pub struct Register {
    servers: Arc<RwLock<HashMap<ServerHostname, Tx>>>,
    client_count: Arc<RwLock<usize>>,
}
/// Used to get rid of DNS caching
pub fn clean_up_hostname(hostname: &str) -> &str {
    let hostname = hostname.trim_end_matches(".");
    if hostname.starts_with("random-") {
        return hostname.split_once('.').map(|x| x.1).unwrap_or("");
    }

    hostname
}

impl Register {
    #[inline]
    pub async fn get_server(&self, hostname: &str) -> Option<Tx> {
        self.servers.read().await.get(hostname).cloned()
    }
    #[inline]
    pub async fn remove_server(&self, hostname: &str) {
        self.servers.write().await.remove(hostname);
    }

    /// throws a DistributorError::ServerAlreadyConnected if server already connected
    #[inline]
    pub async fn add_server(&self, hostname: &str, tx: Tx) -> anyhow::Result<(), DistributorError> {
        let servers = &mut self.servers.write().await;
        if servers.contains_key(hostname) {
            return Err(DistributorError::ServerAlreadyConnected);
        }
        servers.insert(hostname.to_string(), tx);

        Ok(())
    }

    pub async fn get_server_count(&self) -> usize {
        self.servers.read().await.len()
    }
    pub async fn add_client(&self) {
        let mut clients = self.client_count.write().await;
        *clients = clients.saturating_add(1);
    }

    pub async fn remove_client(&self) {
        let mut clients = self.client_count.write().await;
        *clients = clients.saturating_sub(1);
    }

    pub async fn get_client_count(&self) -> usize {
        *self.client_count.read().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        assert_eq!(
            clean_up_hostname("random-123123.testawe.random.com"),
            "testawe.random.com"
        );
        assert_eq!(
            clean_up_hostname("random-ab12cd.testawe.random.com"),
            "testawe.random.com"
        );
        assert_eq!(
            clean_up_hostname("testawe.random.com"),
            "testawe.random.com"
        );
        assert_eq!(
            clean_up_hostname("randomtestawe.random.com"),
            "randomtestawe.random.com"
        );
        assert_eq!(clean_up_hostname("random-123123"), "");
    }
}
