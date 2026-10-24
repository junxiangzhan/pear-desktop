# Android 版改用 GeckoView 的計畫

狀態：四個階段都已完成（2026-10-24），結果見各階段的「結果」一節。前提是 [可行性驗證](README.md#geckoview-可行性驗證android2026-10-24) 的結果：GeckoView 裡可以登入 Google 並保持登入，WebExtension 能在頁面腳本之前注入主世界。桌面版不受影響，仍用 Tauri 的 WebView2／WebKit。

## 目標與範圍

Android 版把顯示 YouTube Music 的引擎從 Android WebView 換成 GeckoView，讓使用者能登入，同時保留現有的一切：Rust 主行程（設定、歌曲資訊、插件 backend）、頁面裡的 renderer bundle 與插件、背景播放與媒體控制、選單、連結開啟。

不在範圍內：桌面版、iOS、改寫插件本身。

## 整體架構

```
GeckoView（顯示 music.youtube.com）                     Tauri（Android 上只剩 Rust 核心）
 頁面主世界：renderer bundle                              Rust：ipc / config / song_info / plugins …
   tauri.ts invoke() / listen() ── Gecko 後端 ──┐               ▲                 │
 內建 WebExtension                              │               │ Channel         │ run_mobile_plugin("deliver")
   content script（isolated）◀─────────────────┘               │                 ▼
     exportFunction / wrappedJSObject                    Kotlin：GeckoBridgePlugin（Tauri mobile plugin）
     runtime.connectNative("pear") ◀── native messaging ──▶ GeckoActivity 的 MessageDelegate / Port
```

Tauri 的 WebView 仍存在（Tauri 的生命週期需要它），但只載入 `about:blank`、不注入任何東西，也不顯示。

## 1. Tauri 在 Android 上的角色

**保留 Tauri，只換掉顯示用的 WebView。** Rust 主行程到處用 Tauri 的 `AppHandle`、`State`、插件（dialog、notification、clipboard）與 mobile plugin（`MediaSessionPlugin`），不靠 Tauri 等於重寫這些模組。

- `MainActivity` 仍是 `TauriActivity`（Rust 的啟動與生命週期照舊）。`onWebViewCreate` 之後把畫面換成 `GeckoView`；Tauri 的 WebView 留在背景。
- `lib.rs` 在 Android 上把 Tauri 視窗的起始網址設為 `about:blank`，不註冊 `pear` 協定的 init script。
- 目前直接操作 Tauri WebView 的程式改到 GeckoView：
  - `presentAsDesktopChrome`：不再需要，GeckoView 用 `USER_AGENT_MODE_DESKTOP`／`VIEWPORT_MODE_DESKTOP`。
  - 背景播放時讓 WebView 保持「可見」的處理：改成讓 `GeckoSession` 保持 active（需驗證 Gecko 在 activity 停止時是否暫停媒體）。
  - 連結開啟、返回鍵、系統列 inset：改用 `GeckoSession.loadUri`／`goBack` 與 GeckoView 的 padding（驗證版已實作）。
- ~~待確認：背景閒置的 Chromium WebView（`about:blank`）佔多少記憶體。~~ 第 3 階段實測 renderer 只有 5.9 MB，不需要處理。

## 2. IPC 橋接

頁面目前只經過 `inject/tauri.ts` 的 `invoke()` 與 `listen()` 和 Rust 溝通（14 個指令：`ipc_send`、`ipc_invoke`、`config_set`、`config_set_partial`、`config_edit`、`plugins_manifest`、`menu_update`、`menu_set_checked`、`net_fetch`、`app_restart`、`prompt_open`、`window_*`）；Rust 推給頁面只有兩個 `emit_to`（`pear:ipc`、`pear:config`）和兩個 Android 會用到的 `eval`（選單的上一頁／重新整理、開啟搜尋框）。橋接只需要替換這幾個點。

**頁面 → Rust**

1. `tauri.ts` 多一個後端：頁面上有 `window.__pearNative` 時（GeckoView）改走它，其餘程式不變。訊息格式：`{ id, cmd, args }`（`send` 類不帶 `id`）。
2. content script（isolated world）用 `exportFunction` 把 `__pearNative.send()` 放進頁面，收到後經 `browser.runtime.connectNative("pear")` 的 port 送出（需要 `nativeMessaging`、`nativeMessagingFromContent` 權限，驗證版已用）。
3. Kotlin：`GeckoActivity` 的 `MessageDelegate.onConnect` 拿到 `Port`，把訊息交給 `GeckoBridgePlugin`；它沿用 `MediaSessionPlugin` 的做法，用 Rust 建立的 `Channel` 送進 Rust。
4. Rust：新增 `gecko_bridge.rs`（只在 Android 編譯），依 `cmd` 分派到既有的指令函式（它們本來就是 `fn(AppHandle, …)`，直接呼叫即可；Android 沒有的 `window_*`、`prompt_open` 回傳錯誤）。`invoke` 的結果以 `{ id, ok | err }` 回傳。

**Rust → 頁面**

1. `ipc::send_to_page` 與 config 的 `pear:config` 改呼叫同一個 `page_emit(app, event, payload)`：Android 走橋接，桌面照舊 `emit_to`。所有 `send_to_page` 呼叫點（16 處）不用改。
2. `eval` 的兩處改成橋接的 `{ eval }` 訊息，由 content script 在頁面裡執行。
3. Rust 以 `run_mobile_plugin("deliver", msg)` 交給 Kotlin（會阻塞，放在背景執行緒），Kotlin 經 `Port.postMessage` 給 content script，再呼叫頁面上的 `__pearReceive`（`cloneInto` 複製資料）。

**資料量**：每秒的播放時間、設定快照（數 KB）、`net_fetch` 的回應（歌詞、crossfade 的 youtubei.js，最多數百 KB）都是 JSON，走 JNI 沒有問題；這部分在第 2 階段量測延遲。

## 3. bundle 與設定的注入

Tauri 版的 loader 用同步 XHR 從 `pear` 協定拿「設定快照 + bundle」，在任何頁面腳本之前於主世界執行。GeckoView 版要在同樣的時間點提供同樣的東西：

- **bundle**：建置時把 `renderer.js` 放進 extension 的 assets（`assets/pear-gecko/`），由 content script 以同步 XHR 讀取 extension 自己的檔案。
- **設定快照**（`__PEAR_STORED_CONFIG__` 等）會變動，不能放在靜態檔案裡。做法（建議）：
  1. extension 的 background script 啟動時以 native port 向 Rust 要設定，之後每次 `pear:config` 都會收到新的快照，並存到 `browser.storage.local` 供下次啟動。
  2. background script 用 `browser.contentScripts.register({ js: [{ code }], runAt: "document_start" })` 註冊一段**內含設定快照**的 content script，設定改變時重新註冊。這段程式同步地把設定寫進頁面（`window.wrappedJSObject` + `cloneInto`），再讀取 bundle、用頁面自己的 `eval`（配 Trusted Types policy，同 Tauri 版 loader）在主世界執行。設定與 bundle 在同一段程式裡，不必擔心兩個 content script 的先後。
  3. 第一次開啟時，Kotlin 等 extension 回報「設定已就緒」才 `loadUri`（驗證版已是等 extension 安裝完成才載入）。
- `contentScripts.register` 是 Manifest V2 的 API，驗證版用的是 MV3（為了 `world: "MAIN"`）。第 1 階段要先確認 GeckoView 153 的 MV2 內建 extension 支援這條路；不支援時的替代方案：MV3 的主世界 bundle 改以同步 XHR 向 Rust 開的本機 HTTP（`127.0.0.1`，帶隨機 token）取設定，但要處理本機埠對其他 app 開放的問題。

## 4. 其他 Android 功能

| 功能 | GeckoView 版的做法 |
|---|---|
| 背景播放 | `PlaybackService`（foreground service + MediaSession）不變；要驗證 Gecko 在 activity 停止時是否繼續播放 |
| 媒體控制的歌曲資訊 | 先沿用 Rust 的 `android_media.rs`（與引擎無關）。之後可改用 GeckoView 的 `MediaSessionDelegate`：Gecko 有 `navigator.mediaSession`，YTM 會自己填入（WebView 沒有） |
| 廣告阻擋 | do-not-track 的 In player 模式在 bundle 裡，照常運作（驗證版已確認 `JSON.parse` 包裝有效） |
| 選單 | in-app-menu 在 bundle 裡，經橋接取得選單 |
| 連結 | `MainActivity` 收到的連結改交給 `GeckoSession.loadUri` |
| 檔案對話框、通知 | Tauri 插件照舊（與顯示引擎無關） |

## 5. 體積

以 GeckoView 153（arm64）實測：

| 項目 | 未壓縮 | 壓縮後 |
|---|---|---|
| GeckoView 合計 | 192 MB | 90 MB |
| └ `libxul.so`（Gecko 引擎） | 153 MB | 64 MB |
| └ `omni.ja`（Gecko 資源） | 15 MB | 14 MB |
| debug APK（驗證版，含 GeckoView） | 256 MB | — |
| debug APK（WebView 版） | — | 61.6 MB |

- 新版 Android 打包會讓 native library **不壓縮**存放（可直接載入），所以 GeckoView 在裝置上約多佔 **190 MB**，下載量約多 **90 MB**。改用壓縮打包（`useLegacyPackaging`）只會讓安裝時解壓縮出同樣大小，裝置佔用不會變小。
- 主要來源是 Gecko 引擎本身，沒有實際可行的裁減方式；只出 arm64 已經是最小的組合。
- release 版實測見「第 4 階段結果」：APK 213 MB、壓縮後 99 MB。第三輪移植（下載工具、廣告阻擋等）後重新量測：216 MB、壓縮後 101 MB，增加的都在 Rust 函式庫（17 → 19.6 MB）。
- 對照：桌面版 Tauri 18.9 MB（第三輪後；當時 15.9 MB）vs Electron 397 MB 的優勢在 Android 上不成立；Android 版的價值在於能用插件與登入，而不是體積。

## 6. 階段與驗證

| 階段 | 內容 | 完成的判斷 | 粗估 |
|---|---|---|---|
| 1. bundle 在 Gecko 裡跑起來 | bundle 放進 extension；確認 MV2 `contentScripts.register` 與頁面 `eval`（Trusted Types）可行；IPC 先用假的（console） | 頁面載入 bundle、插件 renderer 部分不報錯；列出在 Firefox 上行為不同的插件 | 1 天 |
| 2. IPC 橋接 | `tauri.ts` 的 Gecko 後端、content script、`GeckoBridgePlugin`、`gecko_bridge.rs`、`page_emit` | 設定讀寫、歌曲資訊、選單點擊、`net_fetch`（歌詞）都經橋接運作；量測來回延遲 | 2–3 天 |
| 3. 取代 MainActivity 的 WebView | Tauri WebView 改 `about:blank`、畫面換成 GeckoView、背景播放、連結、返回鍵 | 平板上：登入、背景播放與鎖定畫面控制、連結、選單 | 1–2 天 |
| 4. 全面驗證 | 插件逐一驗證（廣告阻擋、crossfade、歌詞、SponsorBlock 等）、記憶體、release APK 大小 | README 的平板驗證表格全部重測 | 1 天 |

第 1 階段結束時是決策點：如果太多插件在 Firefox 上有問題，再評估是否繼續。

## 第 1 階段結果（2026-10-24，Xiaomi 平板，GeckoView 153）

**bundle 在 Gecko 裡可以跑。** `GeckoActivity` 的內建 extension 改為 MV2：`background.js` 以 `browser.contentScripts.register({ js: [{ code }] })` 註冊一段含設定快照的程式，在 `document_start` 把設定寫進頁面（`wrappedJSObject` + `cloneInto`），再以頁面的 `eval`（Trusted Types policy）執行 `renderer.js`（建置時由 Gradle 從 `src-tauri/inject/` 複製進 extension）。`stub-ipc.js` 代替 Rust 回答 `__TAURI_INTERNALS__`：`peard:get-config` 依插件預設值合併設定，其他 invoke 拒絕並計數。

- MV2 的 `contentScripts.register` 可用，第 3 節的設計成立，不需要替代方案。
- 把**全部插件**打開（37 個，不含 3 個不可用的），GeckoView 與 WebView（Chromium，同一台平板、同一份設定）**載入的插件完全相同：28 個**有 renderer 部分的插件都印出「loaded」，兩邊都沒有載入失敗。
- 在 GeckoView 裡已登入的狀態下照常運作。

**Firefox 上行為不同、需要處理的地方**

| 問題 | 影響 | 處理 |
|---|---|---|
| YTM 給 Firefox 的是 ES5 版本，會載入 `custom-elements-es5-adapter.js`；它的 `define()` 不用 `new` 建構元素，ES2015 class 會丟 `class constructors must be invoked with 'new'` | `pear-trans`（插件 UI 的翻譯文字）12 次錯誤 | ✅ 已修：`src/renderer.ts` 在 bundle 開始時保留原生 `customElements.define`，用它註冊 `pear-trans`；修正後 0 次 |
| Firefox 的 autoplay 政策讓沒有使用者手勢時建立的 `AudioContext` 停在 `suspended`，而 renderer 讓 `<video>` 經過它輸出 | **播放沒有聲音**（影片在播、音訊圖暫停） | ✅ 已修：`src/renderer.ts` 在 `play`、第一次點擊／按鍵時，以及建立後立刻嘗試 `resume()`。實測：允許 autoplay 時播放中為 `running`；第 3 階段 `GeckoActivity` 要預設允許 autoplay（`PermissionDelegate`，與 WebView 版的 `mediaPlaybackRequiresUserGesture = false` 一致） |
| `AudioContext.setSinkId` 不存在 | custom-output-device 無法切換輸出裝置（它會先檢查，不會出錯） | 不處理；Android 上本來就少用 |
| `requestPictureInPicture` 不存在 | picture-in-picture 的「原生 PiP」模式不能用 | 不處理；這個插件的 backend 本來就只有桌面版 |
| `navigator.mediaSession` 存在（WebView 沒有） | — | 機會：媒體控制可直接用 YTM 填入的資訊 |

靜態掃描插件原始碼的其他 Chromium 專屬用法：`-webkit-app-region`（只有桌面版拖曳視窗用得到）、`::-webkit-scrollbar`（同一處另有 `scrollbar-width`，只影響外觀）。`window.navigation`（crossfade）、`adoptedStyleSheets`、`enumerateDevices` 兩邊都有。

其餘只在 GeckoView 出現的錯誤都是 stub 造成的（`get-menu` 等沒有 Rust 回答），或是 Firefox 的無害警告（`unreachable code after return statement`、CSP 的忽略提示）。尚未逐一點開插件的 UI 確認行為（需要 IPC 橋接後才有意義，留到第 4 階段）。

**結論：可以進入第 2 階段（IPC 橋接）。**

## 第 2 階段結果（2026-10-24，Xiaomi 平板，GeckoView 153）

**橋接可以用，Android 版預設改成 GeckoView。** 頁面、extension、Kotlin、Rust 之間的路徑照第 2 節的設計實作，平板上已登入的狀態下驗證：

| 項目 | 結果 |
|---|---|
| 設定快照 | ✅ background script 啟動時以 `gecko_payload` 向 Rust 取設定，註冊 loader 後才載入頁面（含 `options.resumeOnStart` 的起始網址） |
| 歌曲資訊 | ✅ 頁面的 `ipc_send` 到 Rust（`song_info`、媒體通知照常更新） |
| 選單 | ✅ `menu_update` 送出選單，in-app-menu 的選單列能開啟（第 1 階段打不開）；點擊外掛項目會寫入 `config.json`，Rust 推回 `pear:config`／`config-changed`，Clock 外掛即時出現與消失 |
| `net_fetch` | ✅ synced-lyrics 的 5 次查詢都回來了，歌詞面板有同步捲動的歌詞。這首歌的歌詞來源內容本身是亂碼般的文字，WebView 引擎上一模一樣，與橋接無關 |
| Rust 在頁面執行腳本 | ✅ 選單的「重新整理」經 `{ eval }` 執行 `location.reload()` |
| 設定變更後重新載入 | ✅ 啟動時快照裡 Clock 是開的；從選單關掉再重新整理，新頁面沒有 Clock，表示 background script 收到 `pear:config` 後重新註冊了 loader |
| WebView 引擎 | ✅ `--es engine webview` 啟動時照舊（同一首歌、同樣的選單），Tauri 的 IPC 路徑沒有被影響 |

**延遲（頁面送出到收到回覆，含 Rust 處理時間）**：同步的指令（`menu_update`、`plugins_manifest`）平均 23–34 ms；頁面載入期間偶爾到數百 ms。`ipc_invoke` 平均 25–106 ms（含插件 handler 自己的等待），`net_fetch` 340–700 ms（主要是網路）。每趟要經過 Gecko 的內容行程 → 主行程 → Java → JNI → Rust 再回來，Rust 的同步指令跑在 Android UI 執行緒上，所以頁面載入時會和 GeckoView 搶。目前的資料量下沒有可感覺的延遲；樣本數少，第 4 階段再做專門的量測。

**和計畫不同的地方**

- **GeckoView 移進 `MainActivity`（第 3 階段的一部分提前做）。** Rust 只會隨 Tauri 的 activity 啟動，獨立的 `GeckoActivity` 背後沒有 Rust，接不上橋接。現在 `MainActivity` 在 wry 設好 WebView 後把 `GeckoPage`（GeckoView）疊在上面；Tauri 的 WebView 載入 `about:blank`、不注入。連結、返回鍵也改由 `GeckoPage` 處理。`GeckoActivity`、`stub-ipc.js`、`stub-config.json` 已移除。
- **引擎可以切換**：`MainActivity` 在 Rust 啟動前把 `PEAR_ENGINE` 寫進環境變數，`src/page.rs` 依此決定事件、腳本與導覽走 GeckoView 還是 Tauri 的 WebView。方便兩個引擎對照。
- **沒有用 `storage.local` 快取設定**：Rust 與 extension 在同一個行程，啟動時向 Rust 取設定只要約 5 ms（extension 安裝完成到 loader 註冊完成），不需要快取。
- **`invoke` 一律帶 `id`**：`send` 類的指令也會收到回覆，`tauri.ts` 不必分兩種；多出的回覆很小。

**途中修正**：YTM 播放時會要求離開頁面前確認（`beforeunload`），GeckoView 沒有 prompt delegate 時會直接拒絕，選單的重新整理因此沒有作用。`GeckoPage` 現在一律允許離開，和 Electron 版在 `src/index.ts` 對 `will-prevent-unload` 的處理相同。

**之後的修正**

- viewport 改為 `VIEWPORT_MODE_MOBILE`（UA 仍是桌面版）：桌面 viewport 模式至少以 980 CSS px 排版，視窗縮小時不會切換到 YTM 的窄版面。改後浮動視窗（381 dp）的 viewport 是 385 px，全寬時不變（1173 px）。
- 「關於」改成和 Electron 一樣的子選單：in-app-menu 的頂層按鈕只會開子選單，原本直接放在頂層的項目點了沒反應（Windows 開 in-app-menu 時也一樣）。
- Android 的「檢視」選單加了「切換全螢幕」（隱藏系統列，記在 `options.fullscreen`）。

**留給第 3 階段**：背景播放、複製目前網址、從最近任務滑掉後重開、`app_restart`（見「第 3 階段結果」）。

## 第 3 階段結果（2026-10-24，Xiaomi 平板，GeckoView 153）

畫面換成 GeckoView、Tauri WebView 改 `about:blank`、連結與返回鍵已在第 2 階段完成；這個階段補上其餘部分並在平板上驗證（GeckoView 引擎，已登入）。

| 項目 | 結果 |
|---|---|
| 背景播放 | ✅ 按 Home 後兩分鐘內播放位置持續前進（每 30 秒前進 30 秒）並換到下一首；`PlaybackService` 讓行程存活，Gecko 在 activity 停止後不會暫停媒體，不需要 WebView 那種「保持可見」的處理（已改成只在 WebView 引擎時做） |
| 系統媒體控制 | ✅ 背景時經 media session 送出暫停、播放、下一首（`cmd media_session dispatch`，與通知、鎖定畫面按鈕同一條路），頁面都照做 |
| 鎖定畫面控制 | ✅ 使用者在鎖定畫面實際操作，可以使用 |
| 連結 | ✅ 執行中開啟 `music.youtube.com` 連結會載入該頁；冷啟動時的連結取代起始網址。載入新頁面會中斷正在播放的歌（WebView 版相同） |
| 返回鍵 | ✅ 頁面有上一頁時回上一頁；在第一頁時**退到背景繼續播放**（像 YouTube Music app），原本的 `finish()` 會讓 Tauri 結束整個行程、播放中斷，且結束時 Gecko 的音訊執行緒碰到已銷毀的 mutex 而 abort |
| 從最近任務滑掉 | ✅ MIUI 直接結束行程，重開是冷啟動：仍保持登入、頁面與橋接正常 |
| 重新啟動 | ✅ Tauri 的 `restart()` 在 Android 上只會結束行程；改由 `AndroidAppPlugin` 以 `Intent.makeRestartActivityTask` 重新開啟並結束行程（kill，避開上面的 abort）。「導覽列 › 重新啟動應用程式」在 Android 上也顯示了；實測換了行程、仍登入、繼續播放 |
| 複製目前網址 | ✅ 讀 GeckoView 的網址（`onLocationChange`，含 YTM 頁內切換），貼上得到目前的 `watch?v=…` 網址 |

**記憶體（PSS，播放中）**：主行程 302 MB（Rust、Tauri、Gecko 主行程、背景 WebView 的瀏覽器端）、YTM 的內容行程 405 MB、另外兩個 Gecko 內容行程各約 106 MB、GPU 139 MB、utility 74 MB、media 50 MB、crash helper 63 MB，合計約 1.25 GB。背景 `about:blank` WebView 的 renderer 只有 5.9 MB，所以不需要研究讓 wry 不建立 WebView；記憶體主要是 Gecko 本身。

**之後的修正**：左右分割畫面把視窗從寬變窄時，播放頁的專輯圖只剩幾 px 寬。YTM 的窄版播放頁以 inline `margin: auto Xpx` 讓方形專輯圖置中，X 在 resize 當下量測，版面還在切換，量到的容器寬度是舊的（582 px 的視窗量成約 837 px）；之後任何一次 resize 都會重新量對。`inject/shim.ts` 在 resize 結束 500 ms 後再送一次 `resize` 事件。以 `am task resize`（582 → 寬 → 582 px）驗證：margin 從 285 px 回到 158 px，專輯圖恢復 266×266。

## 第 4 階段結果（2026-10-24，Xiaomi 平板，GeckoView 153）

**README 的平板驗證表格以 GeckoView 重測**（見 README「GeckoView 版的平板驗證」）：頁面與注入、播放與背景播放、媒體與鎖定畫面控制、廣告阻擋、狀態列、標題語言、Google 登入（現在可以）、選單、連結、登入狀態保存都通過。

**插件**

| 插件 | 結果 |
|---|---|
| 廣告阻擋（do-not-track，In player） | ✅ 播放熱門 MV（未訂閱 Premium 的帳號）時播出 0 則廣告（`#movie_player` 沒有進入 `ad-showing`），頁面拿到的播放器回應列出 0 則廣告；WebView 版測過原始回應 5/5 都含廣告 |
| SponsorBlock | ✅ Despacito（`kJQP7kiw5Fk`，0–22 s 與 249–282 s 是 `music_offtopic`）：Rust 取得 2 段，開始播放就跳到 22 s；下一首也跳過了開頭 |
| crossfade | ✅ 頁面內的 audio-url backend（youtubei.js + BotGuard 的 PO token）在 Firefox 上可用，約 2 秒取得串流網址；Con Calma（211 s）在 199 s 換到下一首，也就是照預設在結束前 10 秒開始交接。淡入淡出的聽感要用耳朵確認 |
| synced-lyrics | ✅ 第 2 階段已驗證（`net_fetch` 取得歌詞、同步捲動） |
| in-app-menu | ✅ 前幾個階段已驗證（選單、外掛開關、重新整理、複製網址、重新啟動） |

另外第 1 階段確認了 28 個有 renderer 部分的插件在 GeckoView 都能載入。

**途中修正**

- 「需要重新啟動」對話框的「Restart now」直接呼叫 Tauri 的 `restart()`，在 Android 上只會結束行程；改走 `app_controls::restart`（Android 由 `AndroidAppPlugin` 重新開啟）。實測換了行程、沒有當掉。
- Android 編譯的兩個 unused 警告（`menu.rs` 的 `window`、`MAIN_WEBVIEW` 只在桌面版用到）。

**release APK（arm64，GeckoView）**

| 項目 | 大小 |
|---|---|
| APK 檔 | 213 MB（native library 不壓縮存放，可直接載入） |
| └ `libxul.so`（Gecko） | 153 MB |
| └ `libpear_tauri_poc_lib.so`（Rust，release） | 17 MB |
| └ `omni.ja`（Gecko 資源） | 15 MB |
| └ `renderer.js`（bundle，extension 用；Rust 函式庫裡也嵌了一份給 WebView 引擎） | 7 MB |
| 壓縮後（約等於下載量） | 99 MB |
| 安裝後 | APK 原樣保留（213 MB，不解出 native library）；這台平板的檔案系統壓縮後佔 148 MB |

**記憶體（release，播放中，PSS）**：GeckoView 引擎 900 MB；WebView 引擎 521 MB（app 242 MB + WebView renderer 279 MB；未登入）。GeckoView 約多 380 MB。

**結論**：GeckoView 版可以登入、主要插件都正常，代價是 APK 多約 150 MB（下載多約 90 MB）、記憶體多約 380 MB。

## 7. 風險與待確認事項

- **GeckoView 版本與 Kotlin**：154 以後需要 Kotlin 2.4，專案目前是 2.2。GeckoView 每約 4 週出新版（含安全性更新），要跟上就得升級 Kotlin，可能影響 Tauri 產生的 Android 專案與其插件的編譯。
- **插件的 Chromium 依賴**：插件是為 Electron（Chromium）寫的；Web Audio、`requestPictureInPicture`、`navigator.userAgentData` 等在 Firefox 上可能不同。第 1 階段列清單。
- **兩個引擎同處一個 app**：背景的 Chromium WebView 與 Gecko 的記憶體用量要量測。
- **Google 的態度**：目前 GeckoView 被當成 Firefox 而允許登入；之後若改變檢查方式，同樣可能被擋。
- ~~**MV2 API 的可用性**~~：第 1 階段確認可用。
