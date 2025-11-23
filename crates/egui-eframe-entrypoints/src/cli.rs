use clap::Parser;

/// Parses from the command line arguments on native and from GET parameters on web. TODO: Android settings? Just edit at runtime...?
#[allow(dead_code)]
pub fn parse_args<T: Parser>() -> Result<T, clap::Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        T::try_parse()
    }
    #[cfg(target_arch = "wasm32")]
    {
        // On web, parse from URL query parameters
        let location_string = web_sys::window()
            .and_then(|w| w.location().href().ok())
            .unwrap_or_default();

        let mut args = vec!["large-track-viewer".to_string()];

        if let Some(query_start) = location_string.find('?') {
            let query_string = &location_string[query_start + 1..];
            for pair in query_string.split('&') {
                if let Some(eq_pos) = pair.find('=') {
                    let key = &pair[..eq_pos];
                    let value = &pair[eq_pos + 1..];
                    if key.starts_with("cli") {
                        let arg_key = &key[3..]; // Remove "cli" prefix
                        if !arg_key.is_empty() {
                            args.push(format!("--{}", arg_key));
                        }
                        if !value.is_empty() {
                            args.push(value.to_string());
                        }
                    }
                } else {
                    if pair.starts_with("cli") {
                        let arg_key = &pair[3..]; // Remove "cli" prefix
                        if !arg_key.is_empty() {
                            args.push(format!("--{}", arg_key));
                        }
                    }
                }
            }
        }

        T::try_parse_from(args)
    }
}
