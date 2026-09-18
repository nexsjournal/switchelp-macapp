//! 显式运行的原生凭据集成测试，仅访问随机创建的 GPTSwitch 测试条目。
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
#[ignore = "需在目标系统上显式验收凭据库，不在普通单元测试中访问 OS 凭据库"]
fn native_vault_round_trip_and_cleanup() {
    use switch_core::credentials::{SecretVault, SystemVault};
    let service = format!("app.gptswitch.test.{}", uuid::Uuid::new_v4());
    let vault = SystemVault::new(service).unwrap();
    let reference = format!("gptswitch/test/{}/v1", uuid::Uuid::new_v4());
    struct Cleanup<'a>(&'a SystemVault, &'a str);
    impl Drop for Cleanup<'_> { fn drop(&mut self) { let _ = self.0.delete(self.1); } }
    let _cleanup = Cleanup(&vault, &reference);
    assert!(!vault.exists(&reference).unwrap());
    vault.store(&reference, "synthetic-test-secret").unwrap();
    assert!(vault.exists(&reference).unwrap());
    assert_eq!(vault.load(&reference).unwrap().as_deref(), Some("synthetic-test-secret"));
    vault.delete(&reference).unwrap();
    vault.delete(&reference).unwrap();
    assert!(!vault.exists(&reference).unwrap());
}
