// The module calls into libpam, which sudo has loaded already. Naming it
// makes that explicit where the build machine has it; the Flatpak SDK has
// not, and then the module takes libpam's symbols from sudo, as PAM loads it
fn main() {
    let dirs = ["/usr/lib/x86_64-linux-gnu", "/usr/lib64", "/lib/x86_64-linux-gnu", "/usr/lib"];
    if dirs.iter().any(|d| std::path::Path::new(d).join("libpam.so.0").exists()) {
        println!("cargo:rustc-link-arg=-l:libpam.so.0");
    }
}
