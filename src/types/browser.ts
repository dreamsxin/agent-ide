/** CDP `/json/list` 里的一个页面；和 Rust 的 `services::browser::BrowserTab` 对应 */
export interface BrowserTab {
  id: string;
  title: string;
  url: string;
}
