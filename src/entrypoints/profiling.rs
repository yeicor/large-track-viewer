#[cfg(feature = "profiling")]
pub struct ProfilingServer {
    server: Option<puffin_http::Server>,
}

#[cfg(feature = "profiling")]
impl ProfilingServer {
    pub fn start() -> Self {
        puffin::set_scopes_on(true); // tell puffin to collect data

        match puffin_http::Server::new("127.0.0.1:8585") {
            Ok(puffin_server) => {
                tracing::info!(
                    "Profiling enabled, to view: cargo install puffin_viewer && ~/.cargo/bin/puffin_viewer --url 127.0.0.1:8585"
                );

                ProfilingServer {
                    server: Some(puffin_server),
                }
            }
            Err(err) => {
                tracing::error!("Failed to start puffin server: {err}");
                ProfilingServer { server: None }
            }
        }
    }

    pub fn stop(&mut self) {
        puffin::set_scopes_on(false);
        // Dropping the server will close it.
        self.server = None;
    }
}

pub fn profiling_ui(ui: &mut egui::Ui) {
    #[cfg(feature = "profiling")]
    {
        egui::warn_if_debug_build(ui);
        use crate::entrypoints::profiling::ProfilingServer;
        use egui::widgets::Checkbox;
        use std::sync::{Arc, Mutex};
        // Store profiling server state in a static Mutex
        thread_local! {
            static PROFILING_SERVER: Arc<Mutex<Option<ProfilingServer>>> = Arc::new(Mutex::new(None));
        }
        static mut PROFILING_ENABLED: bool = false;
        let mut enabled = unsafe { PROFILING_ENABLED };
        if ui
            .add(Checkbox::new(&mut enabled, "Enable Profiling Server"))
            .changed()
        {
            unsafe {
                PROFILING_ENABLED = enabled;
            }
            PROFILING_SERVER.with(|server| {
                let mut server = server.lock().unwrap();
                if enabled {
                    if server.is_none() {
                        *server = Some(ProfilingServer::start());
                    }
                } else {
                    if let Some(srv) = server.as_mut() {
                        srv.stop();
                    }
                    *server = None;
                }
            });
        }
    }
    #[cfg(not(feature = "profiling"))]
    {
        ui.label("Profiling feature is disabled in this build.");
    }
}
