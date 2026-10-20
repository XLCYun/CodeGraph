//! 公开入口与私有引擎共用的选集边界；字段缺省和 JSON null 具有不同含义。

use serde::{Deserialize, Deserializer};

/// 显式及发现选集共用的文件数量上限；结果字节数另受传输限额约束。
pub const MAX_FILES: usize = 100_000;

/// 字段缺省由调用处的 default 处理；出现字段时只接受数组，明确拒绝 null。
pub fn deserialize_files<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let files = Vec::<T>::deserialize(deserializer)?;
    if files.len() > MAX_FILES {
        return Err(serde::de::Error::custom("文件选集超过数量上限"));
    }
    Ok(Some(files))
}
