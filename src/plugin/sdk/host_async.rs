use std::sync::Arc;
use std::sync::mpsc;

use crate::adapter::{AdapterManager, AdapterPacket};
use crate::observability::Logger;

struct ReplyDispatchRequest {
    adapter_manager: AdapterManager,
    adapter_id: String,
    packet: AdapterPacket,
    logger: Logger,
    plugin_id: String,
}

#[derive(Clone)]
pub(crate) struct PluginHostAsyncExecutor {
    reply_tx: Arc<mpsc::Sender<ReplyDispatchRequest>>,
}

impl PluginHostAsyncExecutor {
    pub(crate) fn new() -> Self {
        let (reply_tx, reply_rx) = mpsc::channel::<ReplyDispatchRequest>();
        std::thread::Builder::new()
            .name("liteyuki-plugin-host-async".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("plugin host async runtime should initialize");
                while let Ok(request) = reply_rx.recv() {
                    runtime.block_on(async {
                        if let Err(err) = request
                            .adapter_manager
                            .send(&request.adapter_id, request.packet)
                            .await
                        {
                            request.logger.warn_in(
                                "plugin.python",
                                format!(
                                    "plugin '{}' onebot reply send failed (adapter={}): {}",
                                    request.plugin_id, request.adapter_id, err
                                ),
                            );
                        }
                    });
                }
            })
            .expect("plugin host async executor thread should spawn");
        Self {
            reply_tx: Arc::new(reply_tx),
        }
    }

    pub(crate) fn dispatch_onebot_reply(
        &self,
        adapter_manager: AdapterManager,
        adapter_id: String,
        packet: AdapterPacket,
        logger: Logger,
        plugin_id: String,
    ) -> Result<(), String> {
        self.reply_tx
            .send(ReplyDispatchRequest {
                adapter_manager,
                adapter_id,
                packet,
                logger,
                plugin_id,
            })
            .map_err(|_| "plugin host async executor is unavailable".to_string())
    }
}
