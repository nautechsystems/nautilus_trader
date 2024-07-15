use super::config::DataEngineConfig;

struct BacktestDataEngine;

impl BacktestDataEngine {
    fn start(config: DataEngineConfig) {
        let DataEngineConfig {
            clients,
            actors,
            req_queue,
            live,
            ..
        } = config;

        while let Some(req) = req_queue.borrow_mut().pop_front() {
            let client = clients.get(&req.client_id);
            let actor = actors.get(&req.actor_id);
            match (client, actor) {
                (Some(client), Some(actor)) => {
                    let resp = client.handle(req);
                    actor.handle(resp)
                }
                _ => {
                    // TODO: log error
                }
            }
        }
    }
}
