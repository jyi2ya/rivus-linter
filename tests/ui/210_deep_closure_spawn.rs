#![allow(non_snake_case)]

fn rvs_deep_closure_spawn_ABIMST() {
    let _c01 = || {
        let _c02 = || {
            let _c03 = || {
                let _c04 = || {
                    let _c05 = || {
                        let _c06 = || {
                            let _c07 = || {
                                let _c08 = || {
                                    let _c09 = || {
                                        let _c10 = || {
                                            let _c11 = || {
                                                let _c12 = || {
                                                    let _c13 = || {
                                                        let _c14 = || {
                                                            let _c15 = || {
                                                                let _c16 = || {
                                                                    let _c17 = || {
                                                                        std::thread::spawn(|| {});
                                                                    };
                                                                    _c17();
                                                                };
                                                                _c16();
                                                            };
                                                            _c15();
                                                        };
                                                        _c14();
                                                    };
                                                    _c13();
                                                };
                                                _c12();
                                            };
                                            _c11();
                                        };
                                        _c10();
                                    };
                                    _c09();
                                };
                                _c08();
                            };
                            _c07();
                        };
                        _c06();
                    };
                    _c05();
                };
                _c04();
            };
            _c03();
        };
        _c02();
    };
    _c01();
}
