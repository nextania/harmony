fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        // res.set_icon("assets/app.ico");
        res.set("FileVersion", env!("CARGO_PKG_VERSION"));
        res.set("FileDescription", &format!("Harmony"));
        res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
        res.set("ProductName", "Harmony");
        res.set("CompanyName", "Nextania Cloud Technologies");
        res.compile().expect("Failed to compile Windows resources");
    }
}
