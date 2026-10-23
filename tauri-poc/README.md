# Tauri v2 移植

Pear Desktop 的 Tauri 版。頁面（music.youtube.com）裡跑的是現有的 renderer（`src/renderer.ts`、各插件的 renderer / preload / menu 部分）；Electron 主行程的工作（設定、歌曲資訊、播放控制、選單、系統匣、插件 backend）則改寫成 Rust。

分工原則：**商業邏輯與作業系統整合在 Rust**；**UI 定義留在 TS**（插件的 `menu()` 範本在頁面裡執行、由 Rust 畫成原生選單；只顯示翻譯過提示框的 backend 也在頁面處理，因為 i18n 在 TS）。

## 架構

```
music.youtube.com 頁面（TS bundle）            Rust（src-tauri/src）
 shim: ipcRenderer ──ipc_send/ipc_invoke──▶  ipc.rs      依 channel 分派給 handler
       listeners  ◀── emit "pear:ipc" ─────    config.rs   擁有 config.json、插件 watcher
 mainConfig 鏡像   ◀── emit "pear:config" ──    song_info.rs / song_controls.rs / app_controls.rs
 menu-runtime: plugin menu() → JSON ───────▶  menu.rs     原生選單（核心選單 + Plugins）
       click(commandId) ◀── peard:menu-event ─  tray.rs / window.rs / prompt.rs / dialogs.rs
 crossOriginFetch ──── net_fetch（白名單）──▶  net.rs
                                              plugins/*.rs  插件 backend（Plugin trait）
                                              android_media.rs ──▶ PlaybackService.kt（Android）
```

```
tauri-poc/
├─ inject/                   # 頁面端（打包成單一 IIFE）
│  ├─ entry.ts               # 順序：shim → plugin-stubs → preload-plugins → menu-runtime → plugin-glue → @/renderer
│  ├─ shim.ts                # window.ipcRenderer / mainConfig / electronIs / __pearCrossOriginFetch
│  ├─ ipc.ts、tauri.ts        # 對 Rust 的 IPC（不依賴 @tauri-apps/api）
│  ├─ compat/                # build alias：electron、electron-is、custom-electron-prompt、@/config、@/config/plugins、prompt-options；
│  │                         # discord.ts 取代 discord 選單 import 的 main.ts（`backendProxies`）
│  ├─ plugin-stubs.ts        # 把插件預設設定送給 Rust（plugins_manifest）
│  ├─ preload-plugins.ts     # 在頁面裡跑插件的 preload 部分（沿用 src/loader/preload.ts）
│  ├─ menu-runtime.ts        # 執行插件 menu()、序列化給 Rust、處理點擊；順便送出 Rust 對話框要用的翻譯
│  ├─ plugin-glue.ts         # 只有 UI 的 backend（提示框、PiP 按鍵、無邊框視窗拖曳、like 狀態）
│  └─ crossfade.ts           # crossfade 的 `audio-url`（youtubei.js 在頁面裡跑）
├─ plugin-support.json       # 尚未移植的插件清單（Vite 與 Rust 共用）
├─ placeholder/prompt.html   # 提示框視窗（取代 custom-electron-prompt）
└─ src-tauri/
   ├─ src/                   # Rust 主行程（見上圖）；menu_mobile.rs 是行動版的 `menu`（只保留翻譯）
   ├─ capabilities/          # ytm-remote.json：遠端 origin 只能呼叫這些指令；prompt.json
   └─ gen/android            # `tauri android init` 產生的 Android 專案；MainActivity、PlaybackService、MediaSessionPlugin 是手寫的
```

`src/` 與 `vite-plugins/` 的配合修改（不影響 Electron 行為）：`plugin-loader` 可一次保留多個插件部分並排除指定插件；`virtual:plugins` 可輸出多個 `<mode>Plugins()`；新增 `src/providers/net.ts`（`crossOriginFetch`），synced-lyrics 的 LRCLib / Genius / Megalobiz / YTMusic 改用它；notifications 的純常數移到 `constants.ts`；scrobbler 選單不再 import backend；in-app-menu 在宿主設定 `__pearNoTitleBarOverlay` 時（Tauri 沒有 `titleBarOverlay`）自己畫視窗按鈕。

### 注入方式

1. 一段很小的 init script（所有 frame 都會執行，但只在 top frame 動作）用**同步 XHR** 向自訂協定 `pear` 取得 bundle（Windows/Android 是 `https://pear.localhost/renderer.js`，macOS/Linux 是 `pear://localhost/renderer.js`）。
2. 透過 Trusted Types policy 以間接 `eval` 執行。YTM 的 CSP 允許 `'unsafe-eval'`，但要求 `require-trusted-types-for 'script'`。
3. 這樣執行時機等同 Electron 的 preload：在任何頁面腳本之前、在主世界執行。

**為什麼不直接把 bundle 當 init script：** 實測 WebView2 收到這個 6.6 MB、多行的 bundle 時，browser process 會卡在 100% CPU，整個頁面和 DevTools 都無回應。同樣大小的單行字串則沒問題。

### 設定

Rust 擁有 `config.json`（`config.rs`，`defaults.ts` 的移植）。renderer 會同步讀 `window.mainConfig.get()`，所以 Rust 把設定快照接在 bundle 前面一起送出，頁面保留一份鏡像；寫入一律送到 Rust，Rust 存檔、跑插件 watcher（`enabled` 改變時送 `plugin:enable`/`plugin:unload` 並啟停 Rust backend，再送 `config-changed`），最後廣播 `pear:config` 更新鏡像。插件的預設值由頁面在啟動時送來（`plugins_manifest`），`peard:get-config` 會等它到齊。

### 插件狀態

| 狀態 | 插件 |
|---|---|
| 純 renderer，原本就能用 | album-actions、album-color-theme、ambient-mode、clock、equalizer、navigation、performance-improvement、playback-speed、video-toggle、visualizer 等 |
| backend 已移植到 Rust | sponsorblock、synced-lyrics（`synced-lyrics:fetch`）、lumiastream、tuna-obs、notifications（基本模式）、picture-in-picture、transparent-player、scrobbler、shortcuts（全域／視窗內快捷鍵、媒體鍵）、precise-volume、discord、api-server、amuse、in-app-menu、taskbar-mediacontrol（Windows） |
| backend 改在頁面處理 | captions-selector、quality-changer、music-together（只有 UI，`plugin-glue.ts`）、crossfade（`crossfade.ts`） |
| 尚未移植（強制停用、選單變灰） | auth-proxy-adapter、do-not-track、downloader、touchbar |

- **crossfade：** `audio-url` 改在頁面裡用 youtubei.js 取得串流網址（與 `src/plugins/utils/main/youtubei.ts` 相同的做法：`new Function` 直譯器、BotGuard 在隱藏 iframe 裡跑、PO token minter 快取），跨網域請求經 `net_fetch`（白名單加了 `www.youtube.com`、`jnn-pa.googleapis.com`）。
- **discord：** Rust 直接講 Discord 的本機 IPC（Windows named pipe `discord-ipc-N`、Unix socket），取代 `@xhayper/discord-rpc`；節流、暫停逾時清除、自動重連照搬 `DiscordService`。選單在頁面執行，它 import 的 `main.ts` 在打包時換成 `compat/discord.ts`（透過 IPC 讀連線狀態、連線、清除）。
- **api-server：** axum 重寫，路由、JWT（HS256，與 hono/jwt 互通）、CORS、WebSocket 事件與 Electron 版相同；`AUTH_AT_FIRST` 的 Allow/Deny 對話框用頁面送來的翻譯。沒有移植 `/doc`（OpenAPI）與 `/swagger`。預設 secret 是頁面載入時的 `Date.now()`，Rust 第一次啟動時把它存進設定，token 才不會在重新整理後失效。HTTPS 用 rustls（ring）。
- **amuse：** 9863 埠的 `/`、`/query`、`/api`，axum。
- **in-app-menu：** 啟用時（Windows 預設）主視窗無邊框，選單與視窗按鈕由頁面畫，Rust 回答 `get-menu`／`peard:menu-event`／`window-*`。`app-region: drag` 由 `plugin-glue.ts` 轉成 `start_dragging`（雙擊最大化）。window-state 插件不還原 decorations／visible，否則會蓋掉這個設定。
- **系統媒體控制：** Windows 的 SMTC 由 WebView2 自己提供（顯示名稱是 `msedgewebview2.exe`，設定 AppUserModelID 也不會變）；taskbar-mediacontrol 的縮圖按鈕用 `ITaskbarList3`。Linux 的 MPRIS 與 macOS Now Playing 尚未移植（無法在 Windows 上建置測試）。Android 見下方「背景播放」。
- **notifications：** 互動式 Windows toast（附播放按鈕）尚未移植；`interactive` 開啟時退回基本通知。
- **啟用時要求播放器事件：** Electron 的 backend 只在 `peard:player-api-loaded` 時要求 `peard:setup-*-listener`，執行中才啟用的插件會錯過；Rust 版（`plugins::request_player_listeners`）啟動時也會立刻要求一次（頁面端是 singleton，重複要求沒有影響）。

**CSS：** renderer 的 `import 'mdui/mdui.css'` 會被 Vite 抽成獨立 CSS 檔，但 Electron 的 `get-renderer-script` 只讀 `<script>`，所以這份 CSS 在 Electron 裡從未套用。PoC 一度把它內嵌注入，結果 mdui 的 `:root { background-color }` 在 YTM 半透明背景後面畫出灰色色塊；現在改為直接丟棄，行為與 Electron 一致。

### 移植時踩到的坑

- **`shim.ts` 不能 import `virtual:plugins`。** 它會靜態載入所有插件，插件的頂層程式碼會在 shim 定義全域物件之前執行，然後丟 `Cannot read properties of undefined (reading 'on')`。
- **頁面 bundle 不能 import 主行程程式碼。** 選單或 preload 只要間接 import 到 `node:*` 或在載入時呼叫 Electron 的 `app`，整個 bundle 就會在頁面裡崩潰。尚未移植的插件因此由 `plugin-support.json` 排除其 menu/preload；已移植的則在 `src/` 拆開（例如 notifications 的 `constants.ts`）。
- **所有 webview 的 WebView2 環境選項必須相同。** 提示框、Last.fm 登入視窗與主視窗都必須套用同樣的 `use_https_scheme` 與瀏覽器參數（`window::shared_webview_options`），否則建立時會失敗（HRESULT `0x8007139F`）。
- **選單要設在主視窗上，不能用 `app.set_menu`。** app 層級的選單也會掛到提示框視窗，關閉提示框時共用的選單會被拆掉。
- **不要在非主執行緒同步呼叫系統匣／視窗 API。** 歌曲資訊 callback 在 async worker 上執行；直接呼叫 `tray.set_icon` 會等主執行緒，可能互鎖。改用 `run_on_main_thread` 排程。
- **呼叫視窗 API 時不要持有鎖。** `set_position`／`set_size` 會在同一執行緒同步觸發 window event；若 event handler 要取同一把鎖就會死結（PiP 曾因此讓整個 app 無回應）。
- **HTML 全螢幕會變成視窗全螢幕。** Tauri runtime 會把頁面的 fullscreen 直接轉成視窗全螢幕，沒有 Electron `setFullScreenable(false)` 的對應；PiP 在 Resized 事件裡把它復原。
- **`set_size` 設的是內部尺寸。** 要還原尺寸就要存 `inner_size()`，存 `outer_size()` 會每次多出邊框與選單列的高度。
- **選單項目的方法也會等主執行緒。** 在 worker 上呼叫 `CheckMenuItem::is_checked()` 時持有 `checks` 鎖，而主執行緒正在重建選單、要取同一把鎖 → 整個 app 無回應（discord 連線後刷新選單時遇到）。現在一律先把項目 clone 出鎖再呼叫。
- **主行程程式碼要能在行動版編譯。** 選單、系統匣、提示框視窗、視窗操作在 Android 上不存在；這些模組或函式以 `cfg(desktop)` 隔開，行動版的 `menu` 只保留頁面送來的翻譯。

## 執行（Windows）

在 repo 根目錄（Node 26 起不再內附 corepack；若 `pnpm` 指向 `C:\Program Files\nodejs` 裡失效的 corepack shim，請改用獨立安裝的 pnpm 11）：

```bash
pnpm exec vite build -c tauri-poc/vite.inject.config.mts
```

```bash
cargo run --manifest-path tauri-poc/src-tauri/Cargo.toml
```

| 環境變數 | 作用 |
|---|---|
| `PEAR_POC_DEVTOOLS=1` | 啟動時開 DevTools（看 `[pear-tauri-poc]` log） |
| `PEAR_POC_USER_AGENT=...` | 覆寫 User-Agent（例如 Google 登入被擋時測試） |
| `PEAR_POC_NO_INJECT=1` | 不注入，載入原始網站（區分外殼問題與 renderer 問題） |
| `PEAR_POC_INJECT_FILE=path` | 改注入磁碟上的檔案，免重編譯 |
| `PEAR_POC_DEBUG_PORT=9333` | 開啟 Chrome DevTools Protocol（保留 Tauri 預設的 WebView2 參數；不要改用 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`，它會整個覆蓋掉預設參數） |

測試時請正常關閉視窗。強制結束行程會讓 WebView2 來不及寫回 cookie，登入狀態會遺失；若主行程已無回應，先用 CDP 的 `Browser.close` 讓 WebView2 正常關閉再結束行程。另外，單獨按一下 Alt 會讓視窗進入 Windows 的選單模式，期間 WebView2（至少 DevTools）會停止回應，按 Esc 即可離開。

Rust 單元測試：`cargo test --manifest-path tauri-poc/src-tauri/Cargo.toml --lib`（歌曲資訊解析、設定合併、SponsorBlock 片段合併、Last.fm 簽章、快捷鍵名稱轉換、`net_fetch` 白名單等）。

## 執行（Android）

需要 JDK（Android Studio 內附的 JBR 即可）與 NDK：

```bash
export JAVA_HOME="/c/Program Files/Android/Android Studio/jbr" NDK_HOME="$LOCALAPPDATA/Android/Sdk/ndk/28.0.13004108"
```

```bash
cd tauri-poc/src-tauri && pnpm dlx @tauri-apps/cli@2.12.4 android build --debug --apk --target x86_64
```

- `gen/android/gradle.properties` 已加 `android.overridePathCheck=true`，因為 repo 路徑含中文（`D:\程式專案`），AGP 預設會拒絕建置。
- 模擬器若出現 `ERR_NAME_NOT_RESOLVED`（主機有 Tailscale 等 VPN 時常見），用 `emulator -avd <name> -dns-server 8.8.8.8` 啟動。
- 預設使用桌面版 Chrome 的 User-Agent，否則會拿到行動版 YTM，插件無法運作。
- 實機平板請改用 `--target aarch64`。
- debug APK 帶完整除錯資訊時約 374 MB（`.so` 240 MB），模擬器空間不足會裝不上；測試時可在建置前設 `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_STRIP=debuginfo`（約 134 MB）。

### 背景播放

- `PlaybackService`（`mediaPlayback` 類型的 foreground service + `MediaSession`）讓 Android 不會凍結行程，並提供系統媒體控制（通知、鎖定畫面、媒體鍵）。歌曲資訊由 `android_media.rs` 透過 Tauri 的 Android plugin（`MediaSessionPlugin`）送過去；按鈕經 Channel 回到 Rust 變成 `song_controls`。每秒的時間更新只有在跳轉（差超過 2 秒）時才轉送，其餘由 `PlaybackState` 自行推算。
- **只有 foreground service 還不夠：** WebView 被 `onPause()`（`WryActivity` 會呼叫）或視窗變成不可見時，Chromium 會暫停有影像軌的媒體，而 YTM 一律用 `<video>` 播放。`MainActivity` 在 service 執行期間於 `onPause` 後立刻 `webView.onResume()`、`onStop` 後把視窗可見度報回 `VISIBLE`。
- 暫停時通知仍是常駐的（service 不離開前景），避免之後從背景恢復播放時受 Android 12 起的「背景不可啟動前景服務」限制。從最近任務滑掉 app 會停止 service（WebView 跟著 activity 消失）。

## 驗證結果（2026-10-23）

| 項目 | Windows（WebView2 157） | Android 模擬器（API 34, WebView 125） |
|---|---|---|
| 頁面載入、renderer 注入 | ✅ `data-os=Windows`、`player-api-loaded` | ✅ 桌面版 YTM、`data-os=Linux` |
| 插件載入 | ✅ navigation、performance-improvement、playback-speed | ✅（同一份 bundle） |
| 未登入播放 | ✅ 4 秒前進 4.01 秒 | ✅ 前景可播（含廣告） |
| 設定存取（遠端 IPC） | ✅ `config.json` 寫入並於重開後生效 | 未測 |
| 背景播放 | — | ✅（第二輪）按 Home 後持續播放（8 秒前進 8.3 秒、`isFrozen=false`）；修正前按 Home 即暫停、行程被 cached-apps freezer 凍結 |
| Google 登入 | ✅ 預設 UA 可登入；正常關閉後重開仍保持登入（完整的 SID/SAPISID/LOGIN_INFO cookie）。用 `taskkill /F` 強制結束會來不及寫回 cookie，登入狀態會遺失 | 未測 |
| 流暢度：模擬捲動（debug build，未登入） | 原始網站 vs 注入後幾乎相同：靜置 ~165 fps；捲動 ~138 fps、長任務 7 次（兩者皆是 YTM 自身載入內容）。同條件下 Electron 開發版的滾輪測試並沒有比較好 | 靜置 59.3 fps、p95 16.8 ms、無長任務 |
| 流暢度：使用者實際滾輪捲動（已登入，20 秒） | `performance-improvement` **開啟：550 個掉幀**、主執行緒忙碌 76%、`animationiteration` 事件 18,854 次。**關閉：0 個掉幀**、忙碌 22%、無 `animationiteration`。兩者 GC 都很少（Major GC 4–5 次、合計約 60–110 ms），卡頓不是 GC 造成的 | — |
| 同一插件在 Electron vs Tauri（模擬滾輪、插件開啟） | 兩者開銷相近：`animationiteration` 1,806 vs 2,009 次、長任務 3 vs 2 次。WebView2 並沒有放大這個問題 | — |
| 執行檔大小 | 第二輪後重新量測：release exe 15.9 MB（含 7.3 MB bundle；gzip 後 6.0 MB），已安裝的 Electron 版 397 MB（Electron 執行環境本身 355 MB）。WebView2 由 Windows 10/11 內建，不計入。改寫 Rust 主行程之前的 PoC 是 11 MB（壓縮後 3.6 MB） | debug APK 134 MB（不含除錯資訊；含完整除錯資訊時 374 MB，皆非代表值） |

### 卡頓的原因：performance-improvement 的 cpu-tamer

`src/plugins/performance-improvement/scripts/cpu-tamer/cpu-tamer-by-animationframe.js` 會在頁面插入一個隱藏的 `<a-f>` 元素，跑一個 1 ms、無限循環的 CSS 動畫，拿它的 `animationiteration` 事件當時脈（每個 frame 觸發一次），並改寫全域的 `setTimeout`/`setInterval`：媒體播放中時，YTM 的計時器會被延到下一個時脈才執行。在高更新率螢幕（例如 165 Hz）上，主執行緒每個 frame 都要被喚醒一次，捲動時就會掉幀，感覺像是「動一下、卡一下」，容易誤以為是 GC。

**已修正**（[pear-devs/pear-desktop#4772](https://github.com/pear-devs/pear-desktop/pull/4772)，已合併進 `tauri-poc`）：時脈改用 `requestAnimationFrame`，只有在有計時器等待時才要求下一個 frame。計時器的延後與合併行為不變（播放中 1 ms `setInterval` 每秒仍約 164 次）。因此 PoC 不再預設停用這個插件。

修正後在 PoC 重新量測（Windows、WebView2、已登入、未播放、165 Hz、視窗 1100×700 置頂；與 Electron 相同的模擬滾輪：每 33 ms 一格 120 px、每 2 秒反向；每種情況 15 秒 × 2 次取平均；以 `PEAR_POC_INJECT_FILE` 切換 bundle）：

| 情況 | 插件關閉 | 修正前（CSS 動畫時脈） | 修正後（rAF 時脈） |
|---|---|---|---|
| 模擬捲動：掉幀 | 64 | 684 | 73 |
| 模擬捲動：主執行緒忙碌 | 62.9% | 87.3% | 60.3% |
| 靜置：主執行緒忙碌 | 4.3% | 33.0% | 4.3% |
| 靜置：`animationiteration` / main frame | 0 / 4 | 2,555 / 2,518 | 0 / 4 |

播放中的情況沒有在 PoC 量（會寫入帳號的播放紀錄）；Electron 版未登入播放時的量測見 #4772。

## 第一輪移植的驗證結果（Windows，2026-10-23）

以 CDP 與 Win32（讀取 `HMENU`、送 `WM_COMMAND`、`EnumWindows`、DWM 屬性）自動化驗證，使用已登入的設定檔：

| 項目 | 結果 |
|---|---|
| IPC | 未註冊的 `invoke` 會 reject（與 Electron 相同）；`peard:get-config` 合併插件預設值 |
| 歌曲資訊 | 頁面送出 player response → Rust 解析 → `peard:update-song-info` 回到頁面；換歌時更新 |
| 原生選單 | 從選單啟用插件 → 寫入設定、頁面即時載入、選單重建出插件子選單；插件自己的勾選項目與單選群組可用（點擊在頁面執行原本的 handler） |
| 提示框 | Custom window title 與 quality-changer 的提示框：翻譯過的文字、值寫回、取消回傳 `null` / `{response: -1}` |
| 視窗 | Always on top、自訂標題（清空後恢復頁面標題）、單一執行個體（第二次啟動會叫出原視窗） |
| 系統匣 | 從選單開關立即生效；開啟時關閉視窗會隱藏到系統匣；換歌時更新提示文字 |
| 插件 | SponsorBlock（換歌觸發 `sponsorblock-skip`；與 API 結果一致）、synced-lyrics（`synced-lyrics:fetch` 200；`crossOriginFetch` 取 LRCLib 200、白名單外的網域被拒）、全域快捷鍵（實際按鍵 → `peard:toggle-play`、`changeVolume`）、PiP（進出正確、置頂、還原原本位置與大小）、transparent-player（DWM backdrop = Mica） |
| Electron 不受影響 | `pnpm typecheck`、`pnpm lint`、`pnpm build` 通過；Electron 開發版的歌詞供應者照常發出請求 |

未驗證：通知是否實際顯示（沒有錯誤紀錄，但沒有目視確認）、scrobbler 對 Last.fm / ListenBrainz 的實際送出（沒有測試帳號，只有簽章的單元測試）、lumiastream / tuna-obs（需要對應的本機程式）、移植後的捲動流暢度（未重新量測）。

## 第二輪移植的驗證結果（2026-10-23）

| 項目 | 結果 |
|---|---|
| crossfade | `audio-url` 約 290 ms 回傳帶 `pot` 的 googlevideo `audio/mp4` 網址，可播放、可跳到 150 秒；第二次（快取 minter）265 ms |
| 系統媒體控制（Windows） | WebView2 原生 SMTC：WinRT `TryTogglePlayPauseAsync` 可暫停／繼續 |
| taskbar-mediacontrol | 送 `THBN_CLICKED` → `peard:toggle-play`／`previous-video`／`next-video`（按鈕外觀未目視確認） |
| in-app-menu | 無邊框視窗、頁面畫出選單與視窗按鈕、原生選單消失；點選單項目（Always on top）→ 設定與視窗置頂同步；雙擊標題列最大化；`window-maximize`/`unmaximize` 事件送達；關閉按鈕可關閉 |
| discord | 以假的 Discord IPC server（`discord-ipc-0`）驗證：握手 → READY → `SET_ACTIVITY` 內容與 Electron 版格式相同（Listening、按鈕、時間戳、`status_display_type`）；選單顯示「Connected」；Clear activity；server 關閉後狀態變 false、重開後 5 秒內自動重連 |
| api-server | 未帶 token 401、`/auth/{id}`（已授權 client）取得 token、song／volume／like-state／shuffle／repeat-mode／queue/next 回應正確、竄改的 token 401、CORS preflight（含 Private-Network）；WebSocket：`PLAYER_INFO`、暫停／播放的 `PLAYER_STATE_CHANGED`、`VOLUME_CHANGED`、`POSITION_CHANGED`，無 token 以 1008 關閉；重開 app 後自動啟動 |
| amuse | `/` 回傳翻譯文字、`/query` 回傳目前歌曲、CORS |
| Android 背景播放 | 模擬器（API 34）：foreground service（type mediaPlayback）、MediaSession 顯示封面／標題／進度；背景 30 秒持續播放；媒體鍵暫停／播放／下一首在背景有效；正常播放時 30 秒內 0 次 session 更新 |
| Electron 不受影響 | `pnpm typecheck`、`pnpm lint`、`pnpm build` 通過；`cargo test`（24）、`cargo clippy` 無警告；Android 版可編譯 |

未驗證：真正的 Discord 用戶端、api-server 的 Allow/Deny 對話框與 HTTPS、HTTPS 憑證選檔對話框、Android 實機與 Android 上的 api-server／discord。

## 手動驗證清單

- [x] Google 登入成功（Windows，預設 UA）
- [x] 登入後正常關閉、重開仍保持登入（Windows）
- [ ] 確認單獨按 Alt 進入選單模式時，頁面本身是否也會凍結（若是，需攔截 `WM_SYSCOMMAND`/`SC_KEYMENU`）
- [ ] 通知實際顯示、Last.fm 登入流程、ListenBrainz 送出
- [ ] Android 實機平板：登入、播放、觸控操作、背景播放與鎖定畫面控制
- [ ] discord：實際 Discord 用戶端顯示 Rich Presence
- [ ] api-server：新 client 的 Allow/Deny 對話框、HTTPS
- [ ] taskbar-mediacontrol：縮圖按鈕外觀
