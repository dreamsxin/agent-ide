//! 把命令和它的输出在 Windows 和 POSIX 之间对齐。
//!
//! 两个方向各有一类反复出现的失败，而它们都不是"模型不够聪明"能解决的：
//!
//! 1. **写出去的命令**。同一句话在 `sh -lc` 和 `cmd /C` 下含义不同：`FOO=bar npm test` 在
//!    cmd 下会被当成一个叫 `FOO=bar` 的程序；单引号在 cmd 里根本不是引号，`'src/a b.ts'`
//!    会被拆成两个参数；`;` 在 cmd 里是分隔符但写法是 `&`。模型对此的判断依赖它猜没猜对
//!    平台，而猜错的代价是一次失败的运行加一轮返工。能机械翻译的就翻译，翻译不了的**说清
//!    为什么**，而不是把一句注定失败的命令发出去。
//! 2. **读回来的输出**。子进程在中文 Windows 上按活动代码页（GBK/CP936）写字节，而这里
//!    以前一律 `String::from_utf8_lossy`：每个非 ASCII 字节变成 U+FFFD，**在任何人看到
//!    之前就不可逆地毁掉了**。受害者不只是模型 —— Problems 面板按路径和消息解析这段文本，
//!    修复循环把它当作失败原因发回给模型。
//!
//! 参考实现在第 2 点上的做法这里照搬了一半：先按 UTF-8 试，失败再按平台代码页解码，并留
//! 一个环境变量覆盖。不照搬的是"读 `chcp` 的输出"—— 那要每次多起一个进程，而 `GetACP()`
//! 是一次系统调用。第 1 点它没有做（它在 Windows 上换成 Git Bash，把问题挪给了安装环境）。

/// 覆盖子进程输出编码的环境变量。
///
/// 存在的理由：代码页说的是"系统默认"，而一个工具链可以自己决定写别的（MSVC 按代码页，
/// Node 按 UTF-8，Python 看 `PYTHONIOENCODING`）。猜错时用户需要一个不改代码的出口。
pub const OUTPUT_ENCODING_ENV: &str = "AGENT_IDE_OUTPUT_ENCODING";

/// 把子进程的字节解成字符串。
///
/// 顺序有意义：**先试 UTF-8**。现代工具链（cargo、node、rustc）即便在 Windows 上也多按
/// UTF-8 写，而代码页解码会把合法的 UTF-8 中文变成一串乱码 —— 反过来错得更难看。只有
/// UTF-8 解不通时才落到平台编码。
pub fn decode_child_output(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    if let Some(encoding) = fallback_encoding() {
        let (text, _, _) = encoding.decode(bytes);
        return text.into_owned();
    }
    String::from_utf8_lossy(bytes).to_string()
}

/// UTF-8 解不通时该用哪种编码。非 Windows 上返回 `None`（那里 UTF-8 是事实标准，
/// 猜一个单字节编码只会把坏字节伪装成正常文本）。
fn fallback_encoding() -> Option<&'static encoding_rs::Encoding> {
    if let Ok(label) = std::env::var(OUTPUT_ENCODING_ENV) {
        if let Some(encoding) = encoding_rs::Encoding::for_label(label.trim().as_bytes()) {
            return Some(encoding);
        }
    }
    #[cfg(windows)]
    {
        encoding_for_code_page(unsafe { windows_sys::Win32::Globalization::GetACP() })
    }
    #[cfg(not(windows))]
    None
}

/// Windows 活动代码页 → 编码。认不出来的返回 `None`，由调用方退回 lossy。
///
/// 只列真实会遇到的那几个：中文（936 / 54936）、繁中（950）、日文（932）、韩文（949）、
/// 西欧和中欧（1252 / 1250）、西里尔（1251 / 866）。表里没有的宁可 lossy，也不要拿一个
/// 猜出来的单字节编码把坏字节"解释"成看起来正常的文字 —— 那种错误没人能发现。
#[cfg_attr(not(windows), allow(dead_code))]
fn encoding_for_code_page(code_page: u32) -> Option<&'static encoding_rs::Encoding> {
    let label: &[u8] = match code_page {
        65001 => b"utf-8",
        936 => b"gbk",
        54936 => b"gb18030",
        950 => b"big5",
        932 => b"shift_jis",
        949 => b"euc-kr",
        1250 => b"windows-1250",
        1251 => b"windows-1251",
        1252 => b"windows-1252",
        1253 => b"windows-1253",
        1254 => b"windows-1254",
        1255 => b"windows-1255",
        1256 => b"windows-1256",
        1257 => b"windows-1257",
        1258 => b"windows-1258",
        866 => b"ibm866",
        874 => b"windows-874",
        _ => return None,
    };
    encoding_rs::Encoding::for_label(label)
}

/// 把一句命令改成当前平台的 shell 真的能执行的形式。
///
/// POSIX 上原样返回：命令就是按那套语法写的。Windows 上做能机械完成的翻译，剩下的返回
/// 一句解释 —— 让模型换个写法，比让它看一段 `'src/a` 找不到文件的报错快得多。
pub fn portable_command(raw: &str) -> Result<String, String> {
    if cfg!(windows) {
        windows_command(raw)
    } else {
        Ok(raw.to_string())
    }
}

/// 当前平台的 shell 事实，以及两边真正不同的那几条规则。
///
/// 放进工具描述里而不是系统提示词：它只在"要写一句命令"的时候有用，而工具描述是模型
/// 决定调用它时一定会读到的那段文字。只说**有区别**的东西 —— `&&`、管道、`2>&1` 两边
/// 一样，写进去只会把真正的区别淹掉。
pub fn shell_brief() -> &'static str {
    if cfg!(windows) {
        "This machine is Windows and the command runs through `cmd /C`. Single quotes are not \
         quoting there (use double quotes), `VAR=value cmd` does not set a variable, `$VAR` does \
         not expand (use `%VAR%`), and there is no `$(...)` substitution. `&&`, `|` and `2>&1` \
         work as usual. Forward slashes in paths are fine."
    } else {
        "This machine is POSIX and the command runs through `sh -lc`, so ordinary shell syntax \
         applies."
    }
}

/// Windows 上的翻译。独立于 `cfg`，这样两个平台的测试都跑得到它。
fn windows_command(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    let (assignments, rest) = split_leading_assignments(trimmed)?;
    let body = rewrite_for_cmd(rest)?;
    if assignments.is_empty() {
        return Ok(body);
    }
    // `set "K=V"` 的引号把值里的空格和 `&` 一起括住；`&&` 而不是 `&`，因为赋值失败就
    // 不该继续跑那条命令
    let prefix = assignments
        .iter()
        .map(|(key, value)| format!("set \"{}={}\" && ", key, value))
        .collect::<String>();
    Ok(format!("{}{}", prefix, body))
}

/// 开头那几个 `KEY=value`，以及余下的命令。
type LeadingAssignments<'a> = (Vec<(String, String)>, &'a str);

/// 摘掉开头的 `KEY=value` 赋值，返回 (赋值, 余下的命令)。
///
/// 只认**开头**那几个：`npm test -- --grep=a=b` 里的 `=` 不是赋值，而位置是唯一能区分
/// 它们的信息。切词必须认引号 —— `MSG='two words' npm run x` 按空格切会把 `'two` 当成
/// 整个赋值，剩下半个引号，然后报一句"引号没闭合"，而命令本身是对的。
fn split_leading_assignments(command: &str) -> Result<LeadingAssignments<'_>, String> {
    let mut assignments = Vec::new();
    let mut rest = command;
    loop {
        let candidate = rest.trim_start();
        let Some(end) = token_end(candidate) else {
            break;
        };
        let (head, tail) = candidate.split_at(end);
        let Some((key, value)) = head.split_once('=') else {
            break;
        };
        if key.is_empty() || !key.chars().all(|ch| ch.is_alphanumeric() || ch == '_') {
            break;
        }
        // 值两侧的引号在 cmd 里要换成 `set "K=V"` 的形式，所以先剥掉
        let value = value.trim_matches('\'');
        if value.contains('"') {
            return Err(format!(
                "cmd cannot set {} to a value containing a double quote. Set it outside the \
                 command, or use a value without quotes.",
                key
            ));
        }
        assignments.push((key.to_string(), value.to_string()));
        rest = tail;
    }
    Ok((assignments, rest.trim_start()))
}

/// 下一个词在哪里结束（认引号）。没有空白就是最后一个词。
fn token_end(text: &str) -> Option<usize> {
    let mut in_single = false;
    let mut in_double = false;
    for (index, ch) in text.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ch if ch.is_whitespace() && !in_single && !in_double => return Some(index),
            _ => {}
        }
    }
    None
}

/// 单引号换成双引号、顶层 `;` 换成 `&`，并挡掉 cmd 下没有对等物的写法。
fn rewrite_for_cmd(command: &str) -> Result<String, String> {
    let mut out = String::with_capacity(command.len());
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                // 单引号在 cmd 里不是引号，只能换成双引号；内容里本来就有双引号时换不了
                out.push('"');
            }
            '"' if !in_single => {
                in_double = !in_double;
                out.push('"');
            }
            '"' if in_single => {
                return Err(
                    "That command mixes single and double quotes, which cmd cannot express. \
                     Rewrite it with double quotes only."
                        .to_string(),
                );
            }
            ';' if !in_single && !in_double => {
                // cmd 用 `&` 表示"接着跑下一条"，`;` 在那里是普通字符
                out.push('&');
            }
            '$' if !in_single && chars.peek() == Some(&'(') => {
                return Err(
                    "cmd has no `$(...)` command substitution. Run the inner command first and \
                     pass its result, or use a command that does not need substitution."
                        .to_string(),
                );
            }
            '`' if !in_single => {
                return Err(
                    "cmd has no backtick command substitution. Run the inner command separately."
                        .to_string(),
                );
            }
            other => out.push(other),
        }
    }
    if in_single || in_double {
        return Err("That command has an unclosed quote.".to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合法的 UTF-8 必须原样通过 —— 按代码页解一遍会把正常的中文变成乱码。
    #[test]
    fn valid_utf8_is_never_reinterpreted() {
        assert_eq!(decode_child_output("测试通过\n".as_bytes()), "测试通过\n");
        assert_eq!(decode_child_output(b"ok"), "ok");
        assert_eq!(decode_child_output(b""), "");
    }

    /// 环境变量指定的编码要能把 GBK 字节解回中文。
    ///
    /// 这是这个模块存在的那个 bug：`from_utf8_lossy` 会把这四个字节变成 U+FFFD，而下游的
    /// Problems 面板、修复提示词、模型看到的都是那份已经毁掉的文本。
    #[test]
    fn declared_encoding_decodes_instead_of_replacing() {
        let _guard = crate::services::workspace::env_test_guard();
        // "测试" 的 GBK 编码
        let gbk = [0xB2u8, 0xE2, 0xCA, 0xD4];
        assert!(String::from_utf8(gbk.to_vec()).is_err());

        std::env::set_var(OUTPUT_ENCODING_ENV, "gbk");
        let decoded = decode_child_output(&gbk);
        std::env::remove_var(OUTPUT_ENCODING_ENV);

        assert_eq!(decoded, "测试");
    }

    /// 认不出来的代码页要落回 lossy，而不是拿一个猜的编码把坏字节解释成正常文字。
    #[test]
    fn an_unknown_code_page_has_no_encoding() {
        assert!(encoding_for_code_page(437).is_none());
        assert!(encoding_for_code_page(1).is_none());
        assert!(encoding_for_code_page(936).is_some());
        assert_eq!(
            encoding_for_code_page(65001).map(|encoding| encoding.name()),
            Some("UTF-8")
        );
    }

    /// 前导赋值是 POSIX 写法里最常见、在 cmd 下最先炸的一种。
    #[test]
    fn leading_assignments_become_set_commands() {
        assert_eq!(
            windows_command("FOO=bar npm test").unwrap(),
            "set \"FOO=bar\" && npm test"
        );
        assert_eq!(
            windows_command("CI=true RUST_LOG=debug cargo test --lib").unwrap(),
            "set \"CI=true\" && set \"RUST_LOG=debug\" && cargo test --lib"
        );
        // 值带空格时引号必须留在 `set` 的形式里
        assert_eq!(
            windows_command("MSG='two words' npm run x").unwrap(),
            "set \"MSG=two words\" && npm run x"
        );
        // 参数里的 `=` 不是赋值：位置是唯一的区分信息
        assert_eq!(
            windows_command("npm test -- --grep=a=b").unwrap(),
            "npm test -- --grep=a=b"
        );
    }

    /// 单引号在 cmd 里不是引号，带空格的路径会被拆成两个参数。
    #[test]
    fn single_quotes_become_double_quotes() {
        assert_eq!(
            windows_command("npx vitest run 'src/a b.test.ts'").unwrap(),
            "npx vitest run \"src/a b.test.ts\""
        );
        // 引号里的 `;` 和 `$(` 是内容，不是语法
        assert_eq!(windows_command("rg 'a;b' src").unwrap(), "rg \"a;b\" src");
        assert_eq!(
            windows_command("rg 'x$(y)' src").unwrap(),
            "rg \"x$(y)\" src"
        );
    }

    /// `;` 在 cmd 里是普通字符，写成 `&` 才是"接着跑"。
    #[test]
    fn top_level_semicolons_become_ampersands() {
        assert_eq!(
            windows_command("npm run build ; npm test").unwrap(),
            "npm run build & npm test"
        );
        // `&&` 两边通用，不能动
        assert_eq!(
            windows_command("npm run build && npm test").unwrap(),
            "npm run build && npm test"
        );
    }

    /// 翻不了的要说清为什么，而不是发一句注定失败的命令出去。
    #[test]
    fn what_cmd_cannot_express_is_refused_with_a_reason() {
        let error = windows_command("echo $(git rev-parse HEAD)").unwrap_err();
        assert!(error.contains("substitution"), "{}", error);

        let error = windows_command("echo `git rev-parse HEAD`").unwrap_err();
        assert!(error.contains("backtick"), "{}", error);

        let error = windows_command("rg 'say \"hi\"' src").unwrap_err();
        assert!(error.contains("double quotes only"), "{}", error);

        let error = windows_command("npx vitest run 'unclosed").unwrap_err();
        assert!(error.contains("unclosed"), "{}", error);
    }

    /// 普通命令一个字都不该被改。翻译层最贵的失败是"它把本来能跑的命令改坏了"。
    #[test]
    fn ordinary_commands_pass_through_untouched() {
        for command in [
            "cargo test --lib",
            "npm test",
            "npx tsc --noEmit",
            "cargo clippy --all-targets -- -D warnings",
            "npm run build 2>&1 | more",
            "rg \"fn main\" src",
        ] {
            assert_eq!(windows_command(command).unwrap(), command, "{}", command);
        }
    }

    /// 两边都要说清 shell 是哪一个 —— 模型只有知道平台才能写对命令。
    #[test]
    fn the_brief_names_the_shell() {
        let brief = shell_brief();
        if cfg!(windows) {
            assert!(brief.contains("cmd /C"), "{}", brief);
            assert!(brief.contains("Single quotes"), "{}", brief);
        } else {
            assert!(brief.contains("sh -lc"), "{}", brief);
        }
    }
}
