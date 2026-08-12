// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

use std::marker::PhantomData;

enum TestError {
    Failed,
}

fn rvs_helper<const N: usize>(values: [Option<usize>; N]) -> Result<(), TestError> {
    let _ = values;
    Ok(())
}

struct Inner<K, V>(PhantomData<(K, V)>);

impl<K, V> Inner<K, V> {
    fn rvs_inner_M<const N: usize>(
        &mut self,
        values: [Option<usize>; N],
    ) -> Result<(), TestError> {
        rvs_helper(values)
    }
}

struct Outer<K, V, S> {
    inner: Inner<K, V>,
    marker: PhantomData<S>,
}

impl<K, V, S> Outer<K, V, S> {
    fn rvs_outer_M<const N: usize>(
        &mut self,
        values: [Option<usize>; N],
    ) -> Result<(), TestError> {
        self.inner.rvs_inner_M(values)
    }
}
