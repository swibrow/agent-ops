use std::process::Command;

fn main() {
    let web_dir = std::path::Path::new("web");
    if !web_dir.join("package.json").exists() {
        return;
    }

    // Install deps if node_modules missing
    if !web_dir.join("node_modules").exists() {
        let status = Command::new("npm")
            .args(["install"])
            .current_dir(web_dir)
            .status()
            .expect("failed to run npm install");
        assert!(status.success(), "npm install failed");
    }

    // Build frontend
    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir(web_dir)
        .status()
        .expect("failed to run npm build");
    assert!(status.success(), "npm run build failed");

    println!("cargo:rerun-if-changed=web/src");
    println!("cargo:rerun-if-changed=web/index.html");
    println!("cargo:rerun-if-changed=web/vite.config.ts");
}
