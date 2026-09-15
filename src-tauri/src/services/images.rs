//! 图片作为模型输入。
//!
//! 只做一件事：把工作区里的一个图片文件变成可以放进请求体的 `ImagePart`。
//!
//! 为什么第一个生产者是"读工作区里的图片"，而不是截屏：截屏需要另一套 Win32 表面和一个
//! PNG 编码器，而"看一眼 `docs/mockup.png` 然后照着实现"这件事今天就有用，且完全落在
//! 已有的工作区边界内 —— 同一条 `resolve_for_agent_read` 规则，不新增任何 OS 权限。
//! 多模态这条线上真正要先验证的是**线格式和能力降级**，用一个便宜的生产者验证它，
//! 比连着两套新东西一起上要可控。

use serde::Serialize;

/// 一张给模型看的图片。
///
/// 只存 base64，不存路径：这个结构会被序列化进请求体，把本地路径发给模型没有意义，
/// 而且是一次不必要的泄露。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ImagePart {
    /// `image/png` 这样的 MIME 类型
    pub media_type: String,
    /// base64（无换行、带 padding）
    pub base64_data: String,
}

impl ImagePart {
    /// OpenAI 兼容的 `image_url` 需要 data URL 形式。
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.media_type, self.base64_data)
    }
}

/// 单张图片的上限。
///
/// 4 MiB 的原始字节 ≈ 5.5 MB 的 base64。再大就不是"看一眼设计图"而是把请求体撑爆：
/// 多数 provider 有 20 MB 左右的请求上限，而一次运行可能带多张图。宁可明确拒绝，
/// 也不要让一次请求以一条难懂的 413 结束。
pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;

/// 一次运行总共能附上的原始图片字节数。
///
/// 单张的上限管不住重复调用：模型可以在一次运行里读十几张图，每张都合规，总出网量
/// 却没有任何东西看着 —— 一轮里能有几次工具调用，所以修复之前这个量是**无界**的，
/// 不是"每轮一张"。上限按**运行**计而不是按轮次计，因为付钱的是整次运行。
/// 16 MiB ≈ 22 MB base64，正好是"四张最大尺寸的图"，够看完一套设计稿。
pub const MAX_RUN_IMAGE_BYTES: usize = 16 * 1024 * 1024;

/// 这次运行还能不能再附一张 `requested` 字节的图。
pub fn check_run_image_budget(used: usize, requested: usize) -> Result<(), String> {
    check_run_image_budget_with_limit(used, requested, MAX_RUN_IMAGE_BYTES)
}

/// 带上限参数的版本，测试用它把边界做成可达的。
///
/// 错误话术要让模型能自己想出下一步（换小图、少读几张），所以三个数字都写出来：
/// 已用、这一张、上限。只说"超了"会让它原地重试。
pub fn check_run_image_budget_with_limit(
    used: usize,
    requested: usize,
    limit: usize,
) -> Result<(), String> {
    if used.saturating_add(requested) <= limit {
        return Ok(());
    }
    Err(format!(
        "This run has already attached {} bytes of images; adding {} more would pass the {} byte per-run limit. Attach fewer or smaller images.",
        used, requested, limit
    ))
}



/// 从扩展名判断 MIME 类型。
///
/// 只认这四种：它们是所有主流 provider 都接受的交集。返回 `None` 时调用方应该拒绝，
/// 而不是猜一个 —— 猜错的结果是 provider 报一个和图片无关的错误。
pub fn media_type_for(path: &str) -> Option<&'static str> {
    let lower = path.to_lowercase();
    if lower.ends_with(".png") {
        Some("image/png")
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if lower.ends_with(".gif") {
        Some("image/gif")
    } else if lower.ends_with(".webp") {
        Some("image/webp")
    } else {
        None
    }
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 标准 base64（带 padding，无换行）。
///
/// 自己写而不是加一个 crate：这是二十行确定的代码，而 `base64` 的版本演进过 API
/// （0.13 → 0.21 的 `Engine` 改动）。一个依赖换二十行不值得。
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_ALPHABET[(triple >> 18) as usize & 0x3F] as char);
        out.push(BASE64_ALPHABET[(triple >> 12) as usize & 0x3F] as char);
        // 不足 3 字节时补 `=`：解码方靠它知道原始长度
        if chunk.len() > 1 {
            out.push(BASE64_ALPHABET[(triple >> 6) as usize & 0x3F] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(BASE64_ALPHABET[triple as usize & 0x3F] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// 把一段图片字节变成 `ImagePart`。
///
/// 尺寸和类型都在这里判，返回的错误是给模型看的：说清是哪一条限制，它才可能改用别的
/// 办法（比如让用户压缩），而不是原样重试。
pub fn image_part_from_bytes(path: &str, bytes: &[u8]) -> Result<ImagePart, String> {
    let media_type = media_type_for(path).ok_or_else(|| {
        format!(
            "{} is not an image type the model can read (png, jpg, gif, webp).",
            path
        )
    })?;
    if bytes.is_empty() {
        return Err(format!("{} is empty.", path));
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "{} is {} bytes, over the {} byte limit for one image.",
            path,
            bytes.len(),
            MAX_IMAGE_BYTES
        ));
    }
    Ok(ImagePart {
        media_type: media_type.to_string(),
        base64_data: base64_encode(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_examples() {
        // RFC 4648 的测试向量，包含两种 padding 长度
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_bytes_that_are_not_text() {
        // 图片是二进制，高位字节必须原样进出；只用 ASCII 测过的实现会在这里露馅
        assert_eq!(base64_encode(&[0x00, 0xFF, 0x80]), "AP+A");
        assert_eq!(base64_encode(&[0xFB, 0xFF]), "+/8=");
    }

    #[test]
    fn only_the_types_every_provider_accepts_are_recognised() {
        assert_eq!(media_type_for("a/b/mock.PNG"), Some("image/png"));
        assert_eq!(media_type_for("x.jpeg"), Some("image/jpeg"));
        assert_eq!(media_type_for("x.jpg"), Some("image/jpeg"));
        assert_eq!(media_type_for("x.webp"), Some("image/webp"));
        // 猜一个类型的结果是 provider 报一个和图片无关的错，所以这里必须是 None
        assert_eq!(media_type_for("x.bmp"), None);
        assert_eq!(media_type_for("x.svg"), None);
        assert_eq!(media_type_for("noextension"), None);
    }

    #[test]
    fn the_error_says_which_limit_was_hit() {
        let too_big = vec![0u8; MAX_IMAGE_BYTES + 1];
        let error = image_part_from_bytes("big.png", &too_big).unwrap_err();
        assert!(error.contains("limit"), "{}", error);
        // 模型要能从错误里看出该怎么办，所以尺寸也要写出来
        assert!(error.contains(&MAX_IMAGE_BYTES.to_string()), "{}", error);

        let error = image_part_from_bytes("notes.txt", b"hello").unwrap_err();
        assert!(error.contains("png, jpg, gif, webp"), "{}", error);

        let error = image_part_from_bytes("empty.png", b"").unwrap_err();
        assert!(error.contains("empty"), "{}", error);
    }

    #[test]
    fn a_data_url_carries_the_media_type() {
        let part = image_part_from_bytes("mock.png", b"foobar").unwrap();
        assert_eq!(part.media_type, "image/png");
        assert_eq!(part.data_url(), "data:image/png;base64,Zm9vYmFy");
    }

    /// 每张图都合规、总量却失控，是单张上限管不到的那一半。
    ///
    /// 边界要能用满：正好等于上限必须放行，否则最后一张合规的图会被莫名其妙地拒掉。
    /// 三个数字用**互不相同**的值来断言 —— 用已用量等于上限那种对称情形，一个子串
    /// 能同时满足两个断言，于是"三个数字都写出来"这句话可以在只写两个的情况下通过。
    #[test]
    fn the_run_budget_allows_exactly_the_limit_and_refuses_one_byte_more() {
        assert!(check_run_image_budget(0, MAX_RUN_IMAGE_BYTES).is_ok());
        assert!(check_run_image_budget(MAX_RUN_IMAGE_BYTES - 1, 1).is_ok());
        assert!(check_run_image_budget(MAX_RUN_IMAGE_BYTES, 1).is_err());

        let error = check_run_image_budget_with_limit(100, 4096, 1000).unwrap_err();
        assert!(error.contains("100 bytes"), "{}", error);
        assert!(error.contains("4096 more"), "{}", error);
        assert!(error.contains("1000 byte per-run limit"), "{}", error);
        assert!(error.contains("Attach fewer or smaller images"), "{}", error);
    }


    /// 溢出不能变成"放行"。`used + requested` 用饱和加法，否则一个荒谬的大小
    /// 会绕过预算 —— 上游确实拿不到不可信的 `usize`，但这条断言比推理便宜。
    #[test]
    fn an_absurd_size_does_not_wrap_around_into_allowed() {
        assert!(check_run_image_budget(usize::MAX, usize::MAX).is_err());
    }
}

