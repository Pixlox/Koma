fn main() {
    load_oauth_client("KOMA_ANILIST_CLIENT_ID");
    load_oauth_client("KOMA_MAL_CLIENT_ID");
    tauri_build::build()
}

fn load_oauth_client(name: &str) {
    println!("cargo:rerun-if-env-changed={name}");
    println!("cargo:rerun-if-changed=../.env.local");
    if let Ok(value) = std::env::var(name)
        && !value.trim().is_empty()
    {
        println!("cargo:rustc-env={name}={}", value.trim());
        return;
    }
    let Ok(source) = std::fs::read_to_string("../.env.local") else {
        return;
    };
    for line in source.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == name {
            let value = value.trim().trim_matches(['"', '\'']);
            if !value.is_empty() {
                println!("cargo:rustc-env={name}={value}");
            }
            return;
        }
    }
}
