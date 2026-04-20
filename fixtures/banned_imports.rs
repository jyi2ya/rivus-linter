//! 测试 banned import 检测。
//! 应检测 anyhow, eyre, color_eyre 等被禁 crate。

#![allow(dead_code)]
#![allow(unused_imports)]

// 这些应该被警告
use anyhow::Result;
use anyhow::{Error, Context};
use eyre::Report;
use color_eyre::eyre::Result as EyreResult;

// 这些应该没问题
use std::collections::HashMap;
use thiserror::Error;

pub fn rvs_good_function_ABI() -> anyhow::Result<()> {
    // 使用被禁 crate 应该被检测
    Ok(())
}

fn private_fn_without_rvs_prefix() {
    // 这个私有函数没有 rvs_ 前缀，应该被警告
}

fn rvs_another_private_P() {
    // 这个私有函数有 rvs_ 前缀，应该没问题
    panic!("test");
}

pub fn rvs_public_fn() {
    // 这个有 pub，不需要 rvs_ 前缀检查（因为对外可见）
}

mod inner {
    // mod 内部也应该被检查
    use anyhow::Result;
    
    fn bad_private_fn() {
        // 没有 rvs_ 前缀的私有函数
    }
    
    fn rvs_good_private_M(x: &mut i32) {
        *x += 1;
    }
}
