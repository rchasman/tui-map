//! One bounded background request per layer; the terminal loop never waits on I/O.
use super::Feeds;
use std::{
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub struct Client {
    tx: mpsc::Sender<(u32, Result<String, String>)>,
    rx: mpsc::Receiver<(u32, Result<String, String>)>,
    agent: ureq::Agent,
}
impl Default for Client {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .into();
        Self { tx, rx, agent }
    }
}
impl Client {
    pub fn tick(&mut self, feeds: &mut Feeds, center: (f64, f64)) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        for (id, result) in self.rx.try_iter() {
            feeds.complete(id, result.as_deref().map_err(Clone::clone), now);
        }
        for request in feeds.requests(now, center) {
            let tx = self.tx.clone();
            let agent = self.agent.clone();
            std::thread::spawn(move || {
                let result = (|| -> anyhow::Result<String> {
                    Ok(agent
                        .get(&request.url)
                        .call()?
                        .body_mut()
                        .with_config()
                        .limit(8 * 1024 * 1024)
                        .read_to_string()?)
                })()
                .map_err(|e| e.to_string());
                let _ = tx.send((request.id, result));
            });
        }
    }
}
