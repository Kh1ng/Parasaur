//! Allowance-source scope, separate from the runner that spends it.

pub(crate) struct QuotaScope<'a> {
    pub backend: &'a str,
    pub backend_instance: Option<&'a str>,
    pub quota_pool: Option<&'a str>,
}

impl<'a> QuotaScope<'a> {
    /// An explicit dashboard account pool binds Vibe to that account's source.
    /// Unbound routes and other runners keep their original scope. Applying
    /// this at read time also supports observations stored before the binding.
    pub fn new(
        backend: &'a str,
        backend_instance: Option<&'a str>,
        quota_pool: Option<&'a str>,
    ) -> Self {
        if matches!(backend, "vibe" | "mistral-dashboard")
            && quota_pool.is_some_and(|pool| {
                pool.strip_prefix("mistral-dashboard:")
                    .is_some_and(|account| !account.is_empty())
            })
        {
            return Self {
                backend: "mistral-dashboard",
                backend_instance: quota_pool,
                quota_pool,
            };
        }
        Self {
            backend,
            backend_instance,
            quota_pool,
        }
    }
}
