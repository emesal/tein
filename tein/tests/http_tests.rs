//! integration tests for `(tein http)` module.
//!
//! NOTE: these tests do NOT make real HTTP requests (no network in CI).
//! they verify: module import, error returns on bad URLs, scheme wrapper
//! availability, and sandbox blocking.

#[cfg(feature = "http")]
mod http_integration {
    use tein::{Context, Value};
    use tein::sandbox::Modules;

    fn ctx() -> Context {
        Context::builder().standard_env().build().unwrap()
    }

    #[test]
    fn import_tein_http() {
        let ctx = ctx();
        let result = ctx.evaluate("(import (tein http))");
        assert!(result.is_ok(), "failed to import (tein http): {result:?}");
    }

    #[test]
    fn http_get_bad_url_returns_error_string() {
        let ctx = ctx();
        ctx.evaluate("(import (tein http))").unwrap();
        let result = ctx.evaluate("(http-get \"not-a-url\" '())").unwrap();
        assert!(
            matches!(result, Value::String(_)),
            "expected error string, got {result:?}"
        );
    }

    #[test]
    fn http_request_bad_url_returns_error_string() {
        let ctx = ctx();
        ctx.evaluate("(import (tein http))").unwrap();
        let result = ctx
            .evaluate("(http-request \"GET\" \"not-a-url\" '() #f)")
            .unwrap();
        assert!(
            matches!(result, Value::String(_)),
            "expected error string, got {result:?}"
        );
    }

    #[test]
    fn http_request_with_timeout() {
        let ctx = ctx();
        ctx.evaluate("(import (tein http))").unwrap();
        let result = ctx
            .evaluate("(http-request \"GET\" \"http://127.0.0.1:1\" '() #f 0.5)")
            .unwrap();
        assert!(
            matches!(result, Value::String(_)),
            "expected error string on refused connection, got {result:?}"
        );
    }

    #[test]
    fn convenience_procs_exist() {
        let ctx = ctx();
        ctx.evaluate("(import (tein http))").unwrap();
        // verify all convenience procs are bound (calling them would need network)
        for proc_name in &[
            "http-request",
            "http-get",
            "http-post",
            "http-put",
            "http-delete",
        ] {
            let check = format!("(procedure? {proc_name})");
            let result = ctx.evaluate(&check).unwrap();
            assert_eq!(
                result,
                Value::Boolean(true),
                "{proc_name} should be a procedure"
            );
        }
    }

    #[test]
    fn sandbox_blocks_tein_http() {
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (tein http))");
        assert!(
            result.is_err(),
            "sandboxed context should block (tein http)"
        );
    }
}
