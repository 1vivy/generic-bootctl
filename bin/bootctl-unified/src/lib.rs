//! Serving only: register one boot-control `Service` on every transport the device accepts.
//!
//! How the stock HAL is replaced (overlaying its binary, stopping its service, ...) is the
//! consumer's payload method and is not part of this library; the standalone module's own
//! method lives in this package's binary.

/// Register `IBootControl/default` and serve it; returns only on failure.
///
/// No manifest parsing: the service managers refuse a name the device does not declare. AIDL
/// is tried first (it is what `BootControlClient` prefers whenever it is declared); only if
/// servicemanager refuses it are HIDL 1.2, 1.1 and 1.0 tried against hwservicemanager.
pub fn serve(service: generic_bootctl_core::Service) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "android")]
    {
        let service = std::sync::Arc::new(std::sync::Mutex::new(service));
        if let Err(e) = generic_bootctl_aidl::run(std::sync::Arc::clone(&service)) {
            eprintln!("generic-bootctl: AIDL not served ({e}); trying HIDL");
        }
        for minor in [2, 1, 0] {
            if let Err(e) = generic_bootctl_hidl::run(std::sync::Arc::clone(&service), minor) {
                eprintln!("generic-bootctl: HIDL 1.{minor} not served: {e}");
            }
        }
        Err("no boot-control transport accepted the service".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = service;
        Err("service transport requires Android".into())
    }
}
