//! Small non-validating XML parser: no entities, DTDs or external resources.
use std::{
    fs, io,
    path::{Path, PathBuf},
};
#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<Node>,
}
impl Node {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
    fn values<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> {
        self.children
            .iter()
            .filter(move |n| n.name == key)
            .map(|n| n.text.trim())
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn parse(xml: &str) -> io::Result<Node> {
    let mut stack = vec![Node::default()];
    let mut s = xml;
    while !s.is_empty() {
        if let Some(rest) = s.strip_prefix("<!--") {
            let end = rest
                .find("-->")
                .ok_or_else(|| invalid("unterminated comment"))?;
            s = &rest[end + 3..];
            continue;
        }
        if let Some(rest) = s.strip_prefix("<?") {
            let end = rest
                .find("?>")
                .ok_or_else(|| invalid("unterminated declaration"))?;
            s = &rest[end + 2..];
            continue;
        }
        if s.starts_with("<!") || s.starts_with('&') {
            return Err(invalid("DTD/entities unsupported"));
        }
        if s.starts_with('<') {
            let end = s.find('>').ok_or_else(|| invalid("unterminated tag"))?;
            let tag = s[1..end].trim();
            s = &s[end + 1..];
            if let Some(name) = tag.strip_prefix('/') {
                if stack.len() < 2 {
                    return Err(invalid("unbalanced XML"));
                }
                let n = stack.pop().unwrap();
                if n.name != name.trim() {
                    return Err(invalid("mismatched XML"));
                }
                stack.last_mut().unwrap().children.push(n);
            } else {
                let empty = tag.ends_with('/');
                let tag = tag.trim_end_matches('/').trim();
                let split = tag.find(char::is_whitespace).unwrap_or(tag.len());
                let mut n = Node {
                    name: tag[..split].into(),
                    ..Node::default()
                };
                let mut attrs = tag[split..].trim();
                while !attrs.is_empty() {
                    let eq = attrs
                        .find('=')
                        .ok_or_else(|| invalid("invalid attribute"))?;
                    let key = attrs[..eq].trim();
                    attrs = attrs[eq + 1..].trim_start();
                    let quote = attrs
                        .chars()
                        .next()
                        .ok_or_else(|| invalid("empty attribute"))?;
                    if quote != '\'' && quote != '"' {
                        return Err(invalid("unquoted attribute"));
                    }
                    attrs = &attrs[1..];
                    let end = attrs
                        .find(quote)
                        .ok_or_else(|| invalid("unterminated attribute"))?;
                    n.attrs.push((key.into(), attrs[..end].into()));
                    attrs = attrs[end + 1..].trim_start();
                }
                if empty {
                    stack.last_mut().unwrap().children.push(n);
                } else {
                    stack.push(n);
                }
            }
        } else {
            let end = s.find('<').unwrap_or(s.len());
            let text = &s[..end];
            if text.contains('&') {
                return Err(invalid("entities unsupported"));
            }
            stack.last_mut().unwrap().text.push_str(text);
            s = &s[end..];
        }
    }
    if stack.len() != 1 {
        return Err(invalid("unclosed XML"));
    }
    Ok(stack.pop().unwrap())
}
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Declaration {
    pub aidl: bool,
    pub hidl: Option<u8>,
}
pub fn manifest(xml: &str) -> io::Result<Declaration> {
    let tree = parse(xml)?;
    let mut d = Declaration::default();
    for root in tree.children.iter().filter(|n| n.name == "manifest") {
        for hal in root
            .children
            .iter()
            .filter(|n| n.name == "hal" && n.values("name").any(|v| v == "android.hardware.boot"))
        {
            if hal.attr("override") == Some("true") {
                return Err(invalid(
                    "boot HAL override requires resolved manifest; refusing",
                ));
            }
            let format = hal.attr("format").unwrap_or("hidl");
            let versions: Vec<_> = hal.values("version").collect();
            let mut instances = Vec::new();
            for interface in hal.children.iter().filter(|n| n.name == "interface") {
                if interface.values("name").any(|v| v == "IBootControl") {
                    instances.extend(interface.values("instance").map(str::to_owned));
                }
            }
            for fq in hal.values("fqname") {
                let (prefix, instance) = fq
                    .split_once('/')
                    .ok_or_else(|| invalid("invalid boot fqname"))?;
                if format == "aidl" && prefix == "IBootControl" {
                    instances.push(instance.into());
                } else if format == "hidl" {
                    let (ver, iface) = prefix
                        .split_once("::")
                        .ok_or_else(|| invalid("invalid HIDL fqname"))?;
                    if iface == "IBootControl" {
                        hidl_version(ver.trim_start_matches('@'), &mut d)?;
                        instances.push(instance.into());
                    }
                } else {
                    return Err(invalid("unsupported boot interface"));
                }
            }
            if instances.is_empty() || instances.iter().any(|s| s != "default") {
                return Err(invalid("only IBootControl/default supported"));
            }
            match format {
                "aidl" => {
                    if versions.iter().any(|v| *v != "1") {
                        return Err(invalid(
                            "unsupported AIDL boot version (only frozen V1 exists)",
                        ));
                    }
                    d.aidl = true;
                }
                "hidl" => {
                    if !hal.values("transport").any(|v| v == "hwbinder") {
                        return Err(invalid("HIDL passthrough declaration unsupported"));
                    }
                    for v in versions {
                        hidl_version(v, &mut d)?;
                    }
                    if d.hidl.is_none() {
                        return Err(invalid("missing HIDL version"));
                    }
                }
                _ => return Err(invalid("unsupported boot transport")),
            }
        }
    }
    Ok(d)
}
fn hidl_version(v: &str, d: &mut Declaration) -> io::Result<()> {
    let minor = match v {
        "1.0" => 0,
        "1.1" => 1,
        "1.2" => 2,
        _ => return Err(invalid("unsupported HIDL boot version")),
    };
    d.hidl = Some(d.hidl.unwrap_or(0).max(minor));
    Ok(())
}
pub fn files(dir: &Path, suffix: &str, output: &mut Vec<PathBuf>) -> io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if entry.file_type()?.is_dir() {
            files(&p, suffix, output)?;
        } else if p.extension().is_some_and(|e| e == suffix) {
            output.push(p);
        }
    }
    Ok(())
}
pub fn discover(root: &Path) -> io::Result<Declaration> {
    let mut paths = Vec::new();
    for partition in ["vendor", "odm", "system", "system_ext", "product"] {
        files(&root.join(partition).join("etc/vintf"), "xml", &mut paths)?;
    }
    let apex = root.join("apex");
    if apex.exists() {
        for entry in fs::read_dir(apex)? {
            files(&entry?.path().join("etc/vintf"), "xml", &mut paths)?;
        }
    }
    let mut declared = Declaration::default();
    for p in paths {
        // A manifest fragment can be a link into a driver-created procfs entry that does not
        // exist yet (Oplus: odm .../network_manifest.xml -> /proc/oplusManifest/...). An absent
        // file declares nothing, so it must not keep the boot HAL from registering; every other
        // read or parse failure stays fatal.
        let text = match fs::read_to_string(&p) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let d = manifest(&text)?;
        declared.aidl |= d.aidl;
        declared.hidl = declared.hidl.max(d.hidl);
    }
    if !declared.aidl && declared.hidl.is_none() {
        return Err(invalid("no android.hardware.boot declared"));
    }
    Ok(declared)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declaration_contract() {
        assert_eq!(manifest("<manifest/>").unwrap(), Declaration::default());
        let aidl = "<manifest><hal format='aidl'><name>android.hardware.boot</name><version>1</version><fqname>IBootControl/default</fqname></hal></manifest>";
        assert!(manifest(aidl).unwrap().aidl);
        assert!(manifest(&aidl.replace("<version>1", "<version>2")).is_err());
        assert!(manifest(&aidl.replace("/default", "/other")).is_err());
        let hidl = "<manifest><hal format='hidl'><name>android.hardware.boot</name><transport>hwbinder</transport><fqname>@1.2::IBootControl/default</fqname></hal></manifest>";
        assert_eq!(manifest(hidl).unwrap().hidl, Some(2));
        assert!(manifest("<manifest><hal></manifest>").is_err());
        assert!(manifest("<!DOCTYPE x><manifest/>").is_err());
    }
    #[test]
    fn absent_manifest_fragment_does_not_hide_the_boot_declaration() {
        let root = std::env::temp_dir().join(format!("vintf-absent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("odm/etc/vintf/manifest");
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink("/nonexistent/network_manifest", dir.join("network_manifest.xml")).unwrap();
        fs::write(
            dir.join("boot.xml"),
            "<manifest><hal format='aidl'><name>android.hardware.boot</name><fqname>IBootControl/default</fqname></hal></manifest>",
        )
        .unwrap();
        assert!(discover(&root).unwrap().aidl);
        fs::write(dir.join("bad.xml"), "<manifest><hal></manifest>").unwrap();
        assert!(discover(&root).is_err(), "an unparsable fragment stays fatal");
        fs::remove_dir_all(&root).unwrap();
    }
}
