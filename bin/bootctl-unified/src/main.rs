mod vintf;
use std::{fs, io, path::Path};
fn error(s: &str) -> io::Error {
    io::Error::other(s)
}
fn install_plan(root: &Path) -> io::Result<()> {
    let d = vintf::discover(root)?;
    if !d.aidl || d.hidl.is_some() {
        return Err(error(
            "HIDL transport is not shipped yet; refusing HIDL-only or mixed declarations",
        ));
    }
    let mut paths = Vec::new();
    for part in ["vendor", "odm", "system", "system_ext", "product"] {
        vintf::files(&root.join(part).join("etc/init"), "rc", &mut paths)?;
    }
    let mut services = Vec::new();
    for path in paths {
        let text = fs::read_to_string(path)?;
        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            let words: Vec<_> = line.split_whitespace().collect();
            if words.first() != Some(&"service") {
                continue;
            }
            if words.len() < 3 {
                return Err(error("malformed init service"));
            }
            let name = words[1];
            let bin = words[2];
            let mut block = String::new();
            while lines.peek().is_some_and(|l| {
                l.trim().is_empty() || l.starts_with(char::is_whitespace) || l.starts_with('#')
            }) {
                block.push_str(lines.next().unwrap());
                block.push('\n');
            }
            let source = root.join(bin.trim_start_matches('/'));
            let content = fs::read(&source).unwrap_or_default();
            let marker = b"android.hardware.boot";
            if !block.contains("android.hardware.boot")
                && !content.windows(marker.len()).any(|s| s == marker)
            {
                continue;
            }
            if block
                .lines()
                .any(|l| l.split_whitespace().next() == Some("critical"))
            {
                return Err(error("boot HAL service is critical; refusing installation"));
            }
            if words.len() != 3 {
                return Err(error(
                    "boot HAL command has arguments; same-path substitution cannot preserve their semantics",
                ));
            }
            if !bin.starts_with('/')
                || !["/vendor/", "/odm/", "/system/", "/system_ext/", "/product/"]
                    .iter()
                    .any(|p| bin.starts_with(p))
                || bin.contains("..")
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            {
                return Err(error("unsafe/unoverlayable service path"));
            }
            if !source.is_file() {
                return Err(error("boot HAL executable missing"));
            }
            services.push((name.to_owned(), bin.to_owned()));
        }
    }
    services.sort();
    services.dedup();
    if services.len() != 1 {
        return Err(error("expected exactly one unambiguous boot HAL service"));
    }
    for (name, bin) in services {
        println!("service|{name}|{bin}");
    }
    for part in ["vendor", "odm", "system"] {
        for arch in ["lib", "lib64"] {
            let dir = root.join(part).join(arch).join("hw");
            if !dir.exists() {
                continue;
            }
            for entry in fs::read_dir(dir)? {
                let p = entry?.path();
                let name = p
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| error("non-UTF8 HAL path"))?;
                if name.starts_with("bootctrl.") && name.ends_with(".so") {
                    if arch == "lib" {
                        return Err(error(
                            "32-bit legacy boot HAL unsupported; refusing partial overlay",
                        ));
                    }
                    if !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                    {
                        return Err(error("unsafe legacy library name"));
                    }
                    println!("legacy||/{part}/{arch}/hw/{name}");
                }
            }
        }
    }
    Ok(())
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--install-plan") {
        install_plan(Path::new(args.get(2).map(String::as_str).unwrap_or("/")))?;
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("--vendor-probe") {
        generic_bootctl_backends::vendor::probe(Path::new(args.get(2).ok_or("missing library")?))?;
        return Ok(());
    }
    if args.len() != 1 {
        return Err("unexpected arguments".into());
    }
    let d = vintf::discover(Path::new("/"))?;
    if !d.aidl || d.hidl.is_some() {
        return Err("HIDL transport is not shipped yet".into());
    }
    #[cfg(target_os = "android")]
    generic_bootctl_aidl::run(generic_bootctl_backends::service()?).map_err(io::Error::other)?;
    #[cfg(not(target_os = "android"))]
    return Err("service transport requires Android".into());
    #[cfg(target_os = "android")]
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("generic-bootctl: {e}");
        std::process::exit(1);
    }
}
