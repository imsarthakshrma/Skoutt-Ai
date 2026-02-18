// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — python_bridge/analytics_bridge.rs
// PyO3 bridge to Python analytics modules
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use pyo3::prelude::*;
use pyo3::types::PyDict;

/// Generate a weekly performance report via Python analytics
pub fn generate_weekly_report_via_python(db_path: &str) -> Result<String> {
    Python::with_gil(|py| {
        let analytics = py.import("python.analytics.performance")
            .map_err(|e| anyhow::anyhow!("Python analytics module not available: {}", e))?;

        let kwargs = PyDict::new(py);
        kwargs.set_item("db_path", db_path)?;

        let result = analytics
            .call_method("generate_report", (), Some(kwargs))
            .map_err(|e| anyhow::anyhow!("Analytics report failed: {}", e))?;

        let report: String = result.extract()
            .map_err(|e| anyhow::anyhow!("Failed to extract report: {}", e))?;

        Ok(report)
    })
}
