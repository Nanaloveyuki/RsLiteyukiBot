use super::*;

#[test]
// 必要测试
fn decode_web_api_registration_normalizes_route_and_methods() {
    Python::with_gil(|py| {
        let tuple = PyTuple::new(
            py,
            vec![
                "/demo/".into_pyobject(py).expect("route").into_any(),
                py.None().into_bound(py),
                vec![" get ".to_string(), "post".to_string()]
                    .into_pyobject(py)
                    .expect("methods")
                    .into_any(),
            ],
        )
        .expect("tuple");
        let registration = decode_python_web_api_registration("demo", tuple.into_any())
            .expect("registration should decode");

        assert_eq!(registration.route, "/demo");
        assert_eq!(registration.methods, vec!["GET", "POST"]);
    });
}

#[test]
// 必要测试
fn decode_web_api_registration_rejects_empty_methods() {
    Python::with_gil(|py| {
        let tuple = PyTuple::new(
            py,
            vec![
                "/demo/".into_pyobject(py).expect("route").into_any(),
                py.None().into_bound(py),
                Vec::<String>::new()
                    .into_pyobject(py)
                    .expect("methods")
                    .into_any(),
            ],
        )
        .expect("tuple");
        let error = match decode_python_web_api_registration("demo", tuple.into_any()) {
            Ok(_) => panic!("empty methods should fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("missing route or methods"));
    });
}
