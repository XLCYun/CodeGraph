//! Python 入口自行绑定固定 vendored 声明，目标版本继续由项目配置决定。
use crate::protocol::{Environment, Input, Limits, Metadata, Platform, Request, SOURCE_REVISION};
use codegraph_engine_common::{
    graph,
    model::CodeGraph,
    public_protocol::{CheckedRequest, MAX_BYTES, PROTOCOL},
};

/// 不发现解释器、虚拟环境或用户 typeshed；直接进入固定 Ruff/ty 数据库。
pub fn analyze(input: &CheckedRequest) -> anyhow::Result<CodeGraph> {
    if let Some(path) = std::env::var_os(codegraph_engine_common::observation::VARIABLE) {
        codegraph_engine_common::observation::destination(
            std::path::Path::new(&path),
            &input.repository,
            std::slice::from_ref(&input.installation),
        )?;
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    let root = executable
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Python 程序没有安装目录"))?
        .to_owned();
    let context = Request {
        protocol: PROTOCOL.into(),
        message_type: "analyze".into(),
        id: input.request.id.clone(),
        input: Input {
            repository_root: input.repository.clone(),
            files: input.request.files.clone(),
            include_tests: input.request.options.include_tests,
            profile: "python-static-v1".into(),
        },
        environment: Environment {
            scratch_root: std::env::temp_dir().canonicalize()?,
            platforms: vec![Platform {
                id: "python-stdlib".into(),
                kind: "python-embedded-stdlib".into(),
                version: "0.0.84".into(),
                root,
                metadata: Metadata {
                    source_revision: SOURCE_REVISION.into(),
                },
            }],
        },
        limits: Limits {
            analysis_timeout_ms: 30 * 60 * 1000,
            max_result_bytes: MAX_BYTES,
            max_diagnostic_bytes: 256 * 1024,
        },
    };
    graph::finish(
        crate::projection::build(&context)?,
        input,
        "python",
        &["python-stdlib".into()],
    )
}
