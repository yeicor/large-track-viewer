use clap::Parser;
#[cfg(target_arch = "wasm32")]
use std::collections::HashMap;

#[cfg(target_arch = "wasm32")]
thread_local! {
    static ENV_MAP: std::cell::RefCell<Option<HashMap<String, String>>> = std::cell::RefCell::new(None);
}

/// Generic function to get environment variable, parsing it to the desired type.
pub fn get_env<T: std::str::FromStr>(key: &str) -> Option<T> {
    #[cfg(target_arch = "wasm32")]
    {
        ENV_MAP.with(|map| {
            map.borrow()
                .as_ref()
                .and_then(|m| m.get(key))
                .and_then(|s| s.parse().ok())
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var(key).ok().and_then(|s| s.parse().ok())
    }
}

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
                    } else if key.starts_with("env") {
                        let env_key = &key[3..]; // Remove "env" prefix
                        if !env_key.is_empty() {
                            ENV_MAP.with(|map| {
                                let mut map = map.borrow_mut();
                                if map.is_none() {
                                    *map = Some(HashMap::new());
                                }
                                if let Some(ref mut m) = *map {
                                    m.insert(env_key.to_string(), value.to_string());
                                }
                            });
                        }
                    }
                } else {
                    if pair.starts_with("cli") {
                        let arg_key = &pair[3..]; // Remove "cli" prefix
                        if !arg_key.is_empty() {
                            args.push(format!("--{}", arg_key));
                        }
                    } else if pair.starts_with("env") {
                        let env_key = &pair[3..]; // Remove "env" prefix
                        if !env_key.is_empty() {
                            ENV_MAP.with(|map| {
                                let mut map = map.borrow_mut();
                                if map.is_none() {
                                    *map = Some(HashMap::new());
                                }
                                if let Some(ref mut m) = *map {
                                    m.insert(env_key.to_string(), "".to_string());
                                }
                            });
                        }
                    }
                }
            }
        }

        T::try_parse_from(args)
    }
}

/// Parses environment variables from GET parameters on web.
#[allow(dead_code)]
pub fn parse_env() {
    #[cfg(target_arch = "wasm32")]
    {
        // On web, parse env from URL query parameters
        let location_string = web_sys::window()
            .and_then(|w| w.location().href().ok())
            .unwrap_or_default();
        println!("location: {:?}", location_string);

        if let Some(query_start) = location_string.find('?') {
            let query_string = &location_string[query_start + 1..];
            for pair in query_string.split('&') {
                if let Some(eq_pos) = pair.find('=') {
                    let key = &pair[..eq_pos];
                    let value = &pair[eq_pos + 1..];
                    if key.starts_with("env") {
                        let env_key = &key[3..]; // Remove "env" prefix
                        if !env_key.is_empty() {
                            ENV_MAP.with(|map| {
                                let mut map = map.borrow_mut();
                                if map.is_none() {
                                    *map = Some(HashMap::new());
                                }
                                if let Some(ref mut m) = *map {
                                    m.insert(env_key.to_string(), value.to_string());
                                }
                            });
                        }
                    }
                } else {
                    if pair.starts_with("env") {
                        let env_key = &pair[3..]; // Remove "env" prefix
                        if !env_key.is_empty() {
                            ENV_MAP.with(|map| {
                                let mut map = map.borrow_mut();
                                if map.is_none() {
                                    *map = Some(HashMap::new());
                                }
                                if let Some(ref mut m) = *map {
                                    m.insert(env_key.to_string(), "".to_string());
                                }
                            });
                        }
                    }
                }
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // On native, environment variables are already set
    }
}
