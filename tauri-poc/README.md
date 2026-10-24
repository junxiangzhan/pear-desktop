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
├─ plugin-support.json       # 強制停用的插件清單（Vite 與 Rust 共用）
├─ placeholder/prompt.html   # 提示框視窗（取代 custom-electron-prompt）
└─ src-tauri/
   ├─ src/                   # Rust 主行程（見上圖）；menu_mobile.rs 是行動版的 `menu`（只保留翻譯）
   ├─ capabilities/          # ytm-remote.json：遠端 origin 只能呼叫這些指令；prompt.json
   └─ gen/android            # `tauri android init` 產生的 Android 專案；MainActivity、PlaybackService、MediaSessionPlugin 是手寫的
```

`src/` 與 `vite-plugins/` 的配合修改（不影響 Electron 行為）：`plugin-loader` 可一次保留多個插件部分並排除指定插件；`virtual:plugins` 可輸出多個 `<mode>Plugins()`；新增 `src/providers/net.ts`（`crossOriginFetch`），synced-lyrics 的 LRCLib / Genius / Megalobiz / YTMusic 改用它；notifications 的純常數移到 `constants.ts`；scrobbler 選單不再 import backend；in-app-menu 在宿主設定 `__pearNoTitleBarOverlay` 時（Tauri 沒有 `titleBarOverlay`）自己畫視窗按鈕；do-not-track 的 pruner 不再對 `null` 等非物件呼叫 `delete`（原本 `JSON.parse('null')` 會丟 TypeError，Electron 版也有這個問題）；歌曲資訊的標題改用 YTM 播放佇列裡顯示的標題（播放器回應的 `videoDetails.title` 可能是別的語言，例如 zh-TW 頁面上孫燕姿的〈我不難過〉是「I Am Fine」；Electron 版也受影響）；`renderer.ts` 的 AudioContext 在播放、第一次互動時 `resume()`（Firefox 的 autoplay 政策會讓它停在 suspended，影片就沒有聲音），`pear-trans` 改用 bundle 開始時保留的原生 `customElements.define` 註冊（YTM 給 Firefox 的 ES5 adapter 無法建構 ES2015 class）；in-app-menu 為了 #1737 在 `html` 上 `scrollbar-color: unset`，連帶讓 YTM 的歌曲選單（iron-dropdown）在比視窗高時出現系統的粗捲軸，還因為捲軸占掉寬度多出橫向捲軸，in-app-menu 自己的選單面板也一樣；現在兩者都用 YTM 的細捲軸、不再橫向捲動；in-app-menu 的 `titlebar.css` 跟上目前的 YTM 版面：側欄抽屜（`[slot=guide]`）與收合後的窄側欄（`[slot=mini-guide]`、`#mini-guide-background`）和導覽列一樣下移標題列的高度（原本依賴 YTM 已不再設定的 `is-bauhaus-sidenav-enabled`，側欄頂端與收合時的「首頁」按鈕會被導覽列蓋住），`ytmusic-app-layout` 改為佔滿 `100vh`（原本扣掉標題列高度，底部留下 32px 空白）；Windows 與 Android 都有這兩個問題。

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
| backend 已移植到 Rust | sponsorblock、synced-lyrics（`synced-lyrics:fetch`）、lumiastream、tuna-obs、notifications、picture-in-picture、transparent-player、scrobbler、shortcuts（全域／視窗內快捷鍵、媒體鍵）、precise-volume、discord、api-server、amuse、in-app-menu、taskbar-mediacontrol（Windows）、auth-proxy-adapter（桌面）、do-not-track 的 With blocklists 模式 |
| backend 分在頁面與 Rust | downloader（頁面找串流，Rust 下載存檔） |
| backend 改在頁面處理 | captions-selector、quality-changer、music-together（只有 UI，`plugin-glue.ts`）、crossfade（`crossfade.ts`）、do-not-track 的 In player 模式 |
| 不支援（強制停用、選單變灰） | touchbar（見「這個環境無法建置測試的平台功能」） |

- **crossfade：** `audio-url` 改在頁面裡用 youtubei.js 取得串流網址（與 `src/plugins/utils/main/youtubei.ts` 相同的做法：`new Function` 直譯器、BotGuard 在隱藏 iframe 裡跑、PO token minter 快取，共用的部分在 `inject/youtubei.ts`），跨網域請求經 `net_fetch`（白名單加了 `www.youtube.com`、`jnn-pa.googleapis.com`）。
- **downloader：** 頁面（`inject/downloader.ts`）用 youtubei.js 取得歌曲資訊、專輯、封面與串流網址（同 crossfade 的 BotGuard／PO token），Rust（`plugins/downloader.rs`）以 10 MB 分段下載、用 lofty 寫入標籤（標題、演出者、專輯、曲序、封面；1280×720 的封面照 Electron 裁成 720×720）後存檔，檔名 `演出者 - 標題.m4a`。**沒有 ffmpeg，所以一律存 YouTube 原始的 AAC（`.m4a`）**，選 mp3 或 Custom 預設也一樣，預設選單最上面有註明。YouTube 給的是分段（DASH）的 MP4，Windows 內建的播放器打得開卻不會播（直接跳到結尾），所以存檔前先改寫成一般的 MP4（`plugins/downloader/remux.rs`，等同 ffmpeg 的 `-c copy`，yt-dlp 也會做這一步），再寫標籤。單曲的下載在每首歌的「⋮」選單最上面（插件加的「下載」，不是 YTM 自己附 P 標記的 Premium 下載）。播放清單、專輯（曲序）、跳過已存在的檔案、下載完成時下載、工作列進度條、錯誤與開始下載的對話框與 Electron 相同。youtubei.js 用的是未登入的工作階段（與 crossfade 相同），所以 Premium 的 256 kbps 格式、年齡限制與私人播放清單（例如「喜歡的音樂」）抓不到。**Android：** 存到共用的「音樂/Pear Desktop」（MediaStore，Android 10 以上不需要權限；Android 8、9 存在 app 自己的音樂資料夾，移除 app 時會一起刪掉），選單裡的資料夾選項會隱藏。選單在頁面執行，`compat/downloader-menu.ts` 包住原本的選單：資料夾選擇改用非同步對話框（`pear:show-open-dialog` 加了 `openDirectory`），`main/index.ts`、`main/utils.ts` 換成 `compat/downloader.ts`。
- **proxy 與 auth-proxy-adapter：** 選項 › 進階新增「設定 Proxy」（`options.proxy`，Electron 的 `--proxy-server`），改完要重新啟動。Windows 在 WebView2 啟動時套用（`proxy_url`），Rust 自己的請求（歌詞、封面、下載等）也走同一個 proxy；Chromium 不能帶 SOCKS 帳密，所以 auth-proxy-adapter 開啟時 WebView2 改連它在本機開的 SOCKS5 中繼（`plugins/auth_proxy_adapter.rs`），由中繼帶帳密連到上游。**Android（GeckoView）** 由 pear-gecko extension 的 `browser.proxy.onRequest` 直接套用，SOCKS 帳密也能帶，改了立即生效，不需要中繼。
- **discord：** Rust 直接講 Discord 的本機 IPC（Windows named pipe `discord-ipc-N`、Unix socket），取代 `@xhayper/discord-rpc`；節流、暫停逾時清除、自動重連照搬 `DiscordService`。選單在頁面執行，它 import 的 `main.ts` 在打包時換成 `compat/discord.ts`（透過 IPC 讀連線狀態、連線、清除）。
- **api-server：** axum 重寫，路由、JWT（HS256，與 hono/jwt 互通）、CORS、WebSocket 事件與 Electron 版相同；`AUTH_AT_FIRST` 的 Allow/Deny 對話框用頁面送來的翻譯。`/doc`（OpenAPI）與 `/swagger` 來自 zod 路由定義，Rust 跑不了，所以由打包步驟產生（`scripts/api-openapi.mts` 在 Vite SSR 裡載入真正的路由模組、把主行程模組換成空殼，照 `backend/main.ts` 註冊後取出 Hono 的回應），Rust 原樣回傳；Swagger UI 和 Electron 一樣從 jsDelivr 載入。預設 secret 是頁面載入時的 `Date.now()`，Rust 第一次啟動時把它存進設定，token 才不會在重新整理後失效。HTTPS 用 rustls（ring）。
- **amuse：** 9863 埠的 `/`、`/query`、`/api`，axum。
- **in-app-menu：** 啟用時（Windows 預設）主視窗無邊框，選單與視窗按鈕由頁面畫，Rust 回答 `get-menu`／`peard:menu-event`／`window-*`。`app-region: drag` 由 `plugin-glue.ts` 轉成 `start_dragging`（雙擊最大化）。window-state 插件不還原 decorations／visible，否則會蓋掉這個設定。Android 上它是唯一的選單：啟動時一律開啟（頁面依 UA 判斷的預設值在「Linux」上是關閉），不畫視窗按鈕（`__pearNoWindowControls`），選單裡也拿掉 Android 用不到的項目（置頂、開機啟動、系統匣、縮放、全螢幕、DevTools、自訂視窗標題、重新啟動／結束）。
- **系統媒體控制：** Windows 的 SMTC 由 WebView2 自己提供（顯示名稱是 `msedgewebview2.exe` 或「未知的應用程式」，設定 AppUserModelID 也不會變）；taskbar-mediacontrol 的縮圖按鈕用 `ITaskbarList3`。Linux 的 MPRIS 與 macOS Now Playing 沒有移植（見「這個環境無法建置測試的平台功能」）。Android 見下方「背景播放」。
- **notifications：** `interactive` 開啟時，Windows 顯示附上一首／播放暫停／下一首按鈕的 WinRT toast，七種 `toastStyle` 都有（`plugins/notifications/toast.rs`，`interactive.ts` 的移植）。按鈕用 toast 自己的 `Activated` 事件在程式內處理（Electron 用 `peard://` 協定啟動第二個實例），所以 app 要在執行中。按鈕有文字（系統匣選單的翻譯，`hideButtonText` 開啟時不顯示；Electron 的判斷式誤用 `toastStyle`，文字從來沒出現過）與圖示，圖示依 Windows 的淺色／深色主題選黑色或白色（Electron 一律黑色，深色主題下看不見）；和 Electron 一樣 5 秒後收起，`refreshOnPlayPause` 會重新顯示，`trayControls` 讓按系統匣圖示顯示／收起 toast（雙擊切換視窗）。和 Electron 不同的地方：標題有做 XML 跳脫（Electron 遇到 `&` 會顯示失敗）、每首歌的封面用不同檔名（Windows 依路徑快取 toast 圖片）、切換模式不必重新啟動。安裝版用 app 的 identifier（安裝程式把它設在開始功能表捷徑上）。未安裝的開發版（`target\debug`、`target\release`）沒有這個捷徑，所以照 Microsoft 對未封裝 app 的做法自己註冊 `<identifier>.dev`：在 `HKCU\Software\Classes\AppUserModelId\` 下寫入顯示名稱「Pear Tauri PoC (dev)」與圖示，toast 就顯示 app 名稱而不是 PowerShell（註冊失敗才退回 PowerShell 的 ID）。要清掉這個登錄機碼：`reg delete HKCU\Software\Classes\AppUserModelId\com.github.pear-devs.tauri-poc.dev /f`。基本通知（tauri-plugin-notification）在開發版仍顯示成 PowerShell。其他平台與 `interactive` 關閉時是基本通知。
- **do-not-track（廣告阻擋）：** 預設的 **In player** 模式完全在頁面裡執行（包住 `JSON.parse` 與 `Response.json`，刪掉播放器回應裡的 `playerAds`／`adPlacements`／`adSlots`），Windows 與 Android 都可用。**With blocklists** 模式改用 Brave 的 adblock-rust（`plugins/do_not_track.rs`），清單與 Electron 相同（tb.json 列出的預設清單，除非 `disableDefaultLists`，加上 `additionalBlockLists`），scriptlet 與 redirect 用 Ghostery 那份 uBlock Origin 資源；`cache` 開啟且沒有額外清單時快取引擎（和 Electron 一樣不會自動更新）。只有預設清單能用 `trusted-*` scriptlet。**Windows** 在主 webview 的 `WebResourceRequested` 比對每個請求（含 service worker 與 iframe），擋下的回空的 403，redirect 回對應的資源；app 自己的 `*.localhost` 不比對。**Android** 由 pear-gecko extension 的 `webRequest`（blocking）透過 native messaging 問 Rust（`dnt_check`）。兩邊的頁面都在 Electron 載入 Ghostery preload 的地方向 Rust 要這個網址的隱藏 CSS（每個 selector 一條規則，瀏覽器不接受的只會丟掉那一條）與 scriptlet。沒有移植的：Ghostery preload 依 DOM 計算的通用 class/id 隱藏、procedural 過濾、`removeparam`。和 Electron 不同，切回 In player 時引擎會卸載（Electron 要停用插件才會）。打包時 `blocker.ts` 與 Ghostery 的 preload 由 `compat/do-not-track.ts` 取代。
- **啟用時要求播放器事件：** Electron 的 backend 只在 `peard:player-api-loaded` 時要求 `peard:setup-*-listener`，執行中才啟用的插件會錯過；Rust 版（`plugins::request_player_listeners`）啟動時也會立刻要求一次（頁面端是 singleton，重複要求沒有影響）。

**CSS：** renderer 的 `import 'mdui/mdui.css'` 會被 Vite 抽成獨立 CSS 檔，但 Electron 的 `get-renderer-script` 只讀 `<script>`，所以這份 CSS 在 Electron 裡從未套用。PoC 一度把它內嵌注入，結果 mdui 的 `:root { background-color }` 在 YTM 半透明背景後面畫出灰色色塊；現在改為直接丟棄，行為與 Electron 一致。

### 這個環境無法建置測試的平台功能

PoC 只在 Windows 與 Android 上建置、測試，以下功能需要 Linux 或 macOS，沒有實作，只記下移植方向：

- **Linux MPRIS（shortcuts 插件的媒體控制）：** Electron 版在 `src/plugins/shortcuts/mpris.ts`。Rust 可用 zbus（已在 lockfile 裡，tauri 的相依）在 session bus 註冊 `org.mpris.MediaPlayer2.*`，屬性與方法對應到 `song_info`（Metadata、PlaybackStatus、Position）與 `song_controls`（Play、Pause、Next、Previous、Seek、Volume、Shuffle、LoopStatus），`Seeked` 訊號接 `peard:seeked`。WebKitGTK 自己是否也會註冊 MPRIS 需要實測，避免出現兩個播放器。
- **macOS Now Playing：** 用 objc2-media-player 的 `MPNowPlayingInfoCenter`（標題、演出者、專輯、封面、長度、目前時間、播放速率）與 `MPRemoteCommandCenter`（播放、暫停、上一首、下一首、拖動進度）。WKWebView 可能也會回報媒體資訊，同樣要實測。
- **Touch Bar（touchbar 插件）：** 不支援。Tauri 沒有 Touch Bar API，Apple 也已停產有 Touch Bar 的機型；`plugin-support.json` 繼續把它強制停用。

### 尚未移植的 Electron 選項

選單裡除了「設定 Proxy」，下列 Electron 選項沒有移植（選單裡不會出現）：覆寫 user agent（`overrideUserAgent`）、停用硬體加速（`disableHardwareAcceleration`）、啟動時重設快取（`autoResetAppCache`）、隱藏選單列（`hideMenu`）、單一實例開關（PoC 一律是單一實例）、自訂主題 CSS（`themes`）、自動更新（`autoUpdates`；PoC 不打算做）。

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
- **主行程程式碼要能在行動版編譯。** 選單、系統匣、提示框視窗、視窗操作在 Android 上不存在；這些模組或函式以 `cfg(desktop)` 隔開。`menu.rs` 兩邊共用：選單樹（給 in-app-menu 的 Electron 格式 JSON）與點擊處理相同，原生選單項目只在桌面版建立，Android 上勾選狀態存在 `CheckItem`。

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
| `PEAR_POC_PROFILE_DIR=path` | WebView 資料與 `config.json` 改放在這個資料夾（例如在不動到登入狀態的情況下測未登入） |
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
- `MainActivity` 把 WebView 呈現為**同版本的桌面版 Chrome**：UA 字串用實際的 WebView 版本，Client Hints（`Sec-CH-UA*`、`navigator.userAgentData`，原本會列出「Android WebView」）透過 `WebSettingsCompat.setUserAgentMetadata` 改成 Google Chrome／Linux／非行動裝置。否則會拿到行動版 YTM（插件無法運作），Google 登入也會以「這個瀏覽器或應用程式可能不安全」拒絕。`X-Requested-With: <套件名稱>` 標頭仍會送出：用來關掉它的 `setRequestedWithHeaderOriginAllowList` 在 WebView 143 上不支援（`REQUESTED_WITH_HEADER_ALLOW_LIST` 為 false）。
- 畫面從 Android 15 起強制延伸到系統列下方；`MainActivity` 依系統列與瀏海的 inset 留邊距，底色用 YTM 的 `#030303`、系統列圖示用淺色。
- **YouTube Music 連結：** manifest 為 `https://music.youtube.com` 宣告了 VIEW intent filter，`MainActivity` 會載入收到的連結（冷啟動時排在 wry 的起始網址之後，執行中則經 `onNewIntent`）。這個 app 無法通過該網域的驗證，所以 Android 12 起不會自動接手：要在「應用程式資訊 › 預設開啟」加入 `music.youtube.com`；若裝了官方 YouTube Music app，它是已驗證的處理者，會優先。桌面版沒有對應功能（Windows 無法替 https 網址註冊處理程式）。
- **Google 登入：** 在 WebView 裡無法登入，PoC 不打算繞過。登入頁本身送出的 POST（`AccountsSignInUi/data/batchexecute`、`browserinfo`）都帶 `X-Requested-With`，而 `shouldInterceptRequest` 拿不到 POST 內容，無法不帶標頭重送；`browserinfo` 也會回傳瀏覽器指紋。WebView 版目前只能未登入使用；可行的方向是換成 GeckoView（見下一節）。
- 實機平板請改用 `--target aarch64`。
- **頁面引擎：** 預設是 GeckoView（Google 登入可用，見「GeckoView 可行性驗證」）。`adb shell am start -n com.github.pear_devs.tauri_poc/.MainActivity --es engine webview` 改回 WebView，`--es engine gecko` 切回；選擇會記住。`MainActivity` 在 Rust 啟動前把它寫進環境變數 `PEAR_ENGINE`（`src/page.rs` 讀取）。下面關於 WebView 的說明只適用於 WebView 引擎。
- **GeckoView 的 viewport：** UA 是桌面版 Firefox（插件需要桌面版網站），但 viewport 用 `VIEWPORT_MODE_MOBILE`，依 YTM 自己的 `<meta name="viewport" content="width=device-width">` 排版，和 WebView 一樣。`VIEWPORT_MODE_DESKTOP` 會把頁面至少排成 980 CSS px 寬，視窗縮小（分割畫面、浮動視窗、手機）時就不會切換到 YTM 的窄版面；平板浮動視窗（381 dp）實測 viewport 為 385 px。
- **浮動視窗：** 這台 MIUI 平板把 app 拖成浮動視窗時，先給手機大小的視窗（420×750 dp），約 0.5 秒後改成**整個平板大小的視窗再縮小顯示**（logcat 的 `mFreeFormScale=0.526`，configuration 回到 1164×777 dp）。所以浮動視窗裡是縮小的平板版面，也拉不回手機比例：這是 MIUI 對這個 app 的浮動視窗做法，不是 app 限制的（以 `am start --windowingMode 5` 開啟時可以是手機比例，頁面也會切到窄版面）。
- **浮動視窗後版面卡在窄版（已處理）：** 播放頁元件消失、側欄出現第二個 YouTube Music 標誌，都是 YTM 的 bug：它用一組 media query 追蹤版面大小級別（-4 到 3），套用時寫成 `size && size >= -1`（側欄常駐）、`size && size >= -2`（播放頁開啟時顯示導覽列與播放列），而級別 0（寬 1150–1363 px，這台平板正好 1173 px）是 falsy，所以從窄版面**直接跳進級別 0** 時什麼都不會更新，版面停在窄版（導覽列、播放列 `visibility: hidden`、側欄維持窄版的覆蓋式抽屜與標頭）。拖曳視窗會先經過級別 -1 所以平常看不到；MIUI 浮動視窗是一次跳過去。Electron 版把窄視窗直接最大化到這個寬度時應該也會遇到。`inject/shim.ts` 包住 `MediaQueryList.addListener`：級別 0 的 query 開始成立時，先對 YTM 的級別 -1 listener 回報一次，100 ms 後才送出級別 0，等同拖曳經過 -1。以 `am task resize`（手機大小載入 → 平板大小 → 手機 → 平板）驗證：側欄常駐、標頭隱藏、導覽列與播放列顯示，縮回窄版時照 YTM 原本的方式隱藏。`MainActivity` 會在 logcat（tag `PearMainActivity`）記錄 configuration 變更，`report.js` 記錄最近的視窗大小、devicePixelRatio 與這組 media query 的變化。
- **全螢幕：** Android 版的「檢視」選單有「切換全螢幕」勾選項，隱藏狀態列與導覽列（三鍵或手勢提示條），從邊緣滑動會暫時顯示；選擇存在 `options.fullscreen`，下次啟動沿用（`src/android_app.rs`、`AndroidAppPlugin.kt`）。
- **init script 執行兩次：** wry 產生的 Android 程式碼會用 `addDocumentStartJavaScript` 注入 init script，但 `RustWebViewClient.onPageStarted` 又對所有沒被 Rust 攔截的頁面再 `evaluateJavascript` 一次，沒有檢查前者是否可用（wry 的 bug；`generated/` 每次建置都會重新產生，不能直接改）。PoC 的 loader 以 `window.__pearRendererLoaded` 確保每個文件只載入一次 bundle；Tauri 自己的腳本仍會跑兩次，console 會出現無害的 `Cannot redefine property`。
- **已知問題：** 偶爾啟動後頁面停在空白（`about:blank`），強制停止再開就會恢復，還沒追查。
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
| do-not-track（In player） | 未登入的獨立設定檔（`PEAR_POC_PROFILE_DIR`／Android 模擬器），每次播 6 首歌，比對 YouTube 送來的原始 `/youtubei/v1/player` 回應與播放器實際拿到的 `getPlayerResponse()`。**Windows：** 關閉時原始回應 6/6 含廣告欄位、播放器拿到 6/6、播出 1 則廣告；開啟時原始回應仍 6/6 含廣告，播放器拿到 **0/6**、播出 **0** 則。**Android：** 開啟時原始回應 5/5 含廣告（第一首來自頁面 HTML），播放器拿到 **0/6**、播出 0 則 |
| Electron 不受影響 | `pnpm typecheck`、`pnpm lint`、`pnpm build` 通過；`cargo test`（24）、`cargo clippy` 無警告；Android 版可編譯 |

未驗證：真正的 Discord 用戶端、api-server 的 Allow/Deny 對話框與 HTTPS、HTTPS 憑證選檔對話框、Android 上的 api-server／discord。

## 第三輪移植的驗證結果（Windows，2026-10-24）

未登入的獨立設定檔（`PEAR_POC_PROFILE_DIR`），用 WebView2 的除錯埠操作頁面。Android 的結果在表格後面。

| 項目 | 結果 |
|---|---|
| proxy | ✅ 帶帳密的 SOCKS5 測試伺服器：auth-proxy-adapter 開啟時頁面經本機中繼載入（31 個連線），密碼錯誤時 0 個連線、21 次驗證失敗；log 裡的密碼顯示為 `***` |
| downloader：單曲 | ✅ 3.8 MB 的 `.m4a` 不到 2 秒；ffprobe 讀到標題、演出者、專輯與 544×544 封面，整首能完整解碼；檔案總管顯示標題、演出者、專輯、長度。改寫成一般 MP4 之前，Windows 內建播放器（Media Foundation）會直接跳到結尾，改寫後正常播放 |
| downloader：專輯 | ✅ 建立專輯資料夾，兩首曲序 1、2 |
| downloader：跳過已存在 | ✅ 重跑專輯與單曲都沒有重新下載 |
| downloader：下載完成時下載 | ✅ 跳到下一首時自動存下前一首 |
| downloader：選單 | ✅ 預設選單有「一律存原始音訊」說明，兩個資料夾選項改用非同步對話框 |
| notifications（互動式） | ✅ 樣式 4（封面、專輯、時間）與樣式 1（256×256 封面）從 Windows 通知歷程讀回，三個圖示按鈕都在，5 秒後收起；七種樣式的 XML 都能被 WinRT 解析。實際點按三個按鈕與系統匣的顯示／收起都正常（使用者確認）；按鈕原本只有黑色圖示、深色主題下看不出功能，已改成有文字並依主題換色。開發版的 toast 原本顯示成 PowerShell，已改為自己註冊 AppUserModelID；系統媒體控制顯示「未知的應用程式」是 WebView2 的行為（見上方系統媒體控制的說明） |
| api-server `/doc`、`/swagger` | ✅ `/doc` 26 個路徑，標題與 bearer 驗證和 Electron 相同；`/swagger` 在瀏覽器顯示全部路由；`/api` 仍需要 token |
| do-not-track With blocklists | ✅ 12 份清單 3 秒建好引擎，之後從快取載入；頁面載入時擋下 doubleclick 的請求；套用 716 條隱藏規則；播放正常 |

**Android（平板 Xiaomi 25091RP04G，GeckoView 153，已登入，debug build）：**

| 項目 | 結果 |
|---|---|
| downloader | ✅ 從歌曲選單下載：存進 MediaStore 的「Music/Pear Desktop/」，MediaStore 讀到標題、演出者、專輯、長度；拉回電腦後 ffprobe 看到 544×544 封面、整首能解碼。再下載一次會覆寫同一筆（沒有產生重複的項目） |
| do-not-track With blocklists | ✅ 平板上 3 秒建好引擎，之後從快取載入；extension 經 native messaging 擋下與 Windows 相同的 doubleclick 請求，診斷報告顯示播出 0 則廣告。修正：preload 原本會等隱藏 CSS 回來（第一次要下載清單，約 3 秒），擋住其他插件的 preload；改成不等待 |
| proxy | ✅ 選項 › 進階 › 設定代理伺服器 出現原生對話框（翻譯的標題、說明、範例），會帶入目前的值；設定帶帳密的 SOCKS5（電腦上的測試伺服器 + `adb reverse`）後不必重開，googlevideo、music.youtube.com、yt3 的連線立即改走 proxy；清空後立即恢復直連 |

## Android 實機平板（Xiaomi 25091RP04G，Android 16，WebView 143，2026-10-24）

arm64 debug APK 61.6 MB（不含除錯資訊）。小米需要在開發者選項開啟「USB 安裝」。

| 項目 | 結果 |
|---|---|
| 頁面、注入 | ✅ 桌面版 YTM，橫向 1163×776 CSS px；bundle 只載入一次 |
| 觸控操作（使用者實測） | ✅ 正常，沒有感覺到卡頓 |
| 播放、背景播放 | ✅ 回到桌面 30 秒前進 30.5 秒；關閉螢幕 60 秒前進 60.3 秒，行程沒被凍結 |
| 媒體控制（使用者實測） | ✅ 鎖定畫面與通知顯示正確，手動與自然切歌都會更新，可從面板調進度、切歌；媒體鍵暫停／播放／下一首有效 |
| 廣告阻擋（do-not-track） | ✅ 原始回應 5/5 含廣告，播放器拿到 0/6、播出 0 則 |
| 狀態列 | ✅ 修正後頁面從狀態列下方開始，底部不被手勢列蓋住（修正前狀態列圖示疊在頁面上） |
| 標題語言 | ✅ 修正後歌曲資訊與媒體控制顯示「我不難過」（修正前是「I Am Fine」） |
| Google 登入 | ❌ 使用者實測：被拒絕（「這不是安全的瀏覽器」）；把 UA 與 Client Hints 改成桌面版 Chrome 後重測仍被拒。登入頁的 POST 都帶 `X-Requested-With`，無法移除（見上方「Google 登入」） |
| 選單 | ✅ in-app-menu 的標題列與選單（Plugins／Options／View／Navigation／About）、沒有視窗按鈕（DOM 檢查；畫面尚待目視確認） |
| YouTube Music 連結 | ✅ 指定本 app 的連結 intent：執行中與冷啟動都會開到該首歌 |
| 登入狀態保存 | 未登入時關閉再開會還原上次的畫面，cookie 與頁面狀態有寫入 |

## GeckoView 版的平板驗證（Xiaomi 25091RP04G，Android 16，GeckoView 153，2026-10-24）

Android 版預設的頁面引擎。release APK（arm64）213 MB，壓縮後 99 MB；記憶體（PSS，播放中）900 MB，WebView 引擎是 521 MB。細節見 [GECKOVIEW-PLAN.md](GECKOVIEW-PLAN.md) 的「第 4 階段結果」。

| 項目 | 結果 |
|---|---|
| 頁面、注入 | ✅ 桌面版 YTM（UA 為桌面版 Firefox），bundle 經內建 extension 在頁面腳本之前載入；版面隨視窗寬度切換（分割畫面、浮動視窗） |
| 播放、背景播放 | ✅ 回到桌面兩分鐘內持續播放並換歌；在第一頁按返回鍵退到背景繼續播放 |
| 媒體控制 | ✅ 通知與鎖定畫面（使用者實測）；media session 的暫停／播放／下一首 |
| 廣告阻擋（do-not-track） | ✅ 熱門 MV 播出 0 則廣告，播放器回應 0 則 |
| SponsorBlock | ✅ 跳過 `music_offtopic` 片段（Despacito 開頭 0–22 s） |
| crossfade | ✅ 串流網址取得成功，結束前 10 秒開始交接 |
| 歌詞（synced-lyrics） | ✅ |
| 狀態列、全螢幕 | ✅ 頁面不被系統列遮住；「檢視 › 切換全螢幕」隱藏系統列 |
| 標題語言 | ✅ 歌曲資訊使用頁面語言的標題 |
| Google 登入 | ✅ 使用者實測可以登入，重開、重新啟動、從最近任務滑掉後仍保持登入 |
| 選單 | ✅ 開啟、外掛開關即時生效、重新整理、複製目前網址、重新啟動、關於 |
| YouTube Music 連結 | ✅ 執行中與冷啟動 |

## 手機驗證（POCO F7 Pro，HyperOS，Android 16，GeckoView 153，2026-10-24）

直向 384×763 CSS px（DPR 3.75），頁面是 YTM 的窄版面。release APK 與平板相同。

| 項目 | 結果 |
|---|---|
| 安裝、啟動 | ✅ 需要開發者選項的「USB 安裝」；以 adb 操作需要「USB 偵錯（安全設定）」 |
| Google 登入 | ✅ 使用者在手機上登入 |
| 選單 | ✅ 選單列一行放得下；**第二層選單原本超出畫面**，已修正（見下） |
| 導覽列 | ⚠️ 384 px 寬時「登入」按鈕被擠到邊界；上一頁／下一頁箭頭來自 Navigation 插件，在「外掛功能」關掉即可（不需重新啟動） |
| 播放、標題語言 | ✅ 〈我不難過〉在第一個事件是「I Am Fine」，約 1.5 秒後更正 |
| 廣告阻擋 | ✅ do-not-track 預設是關的，新安裝會有廣告；開啟後 0 則 |
| 背景播放 | ✅ 修正後背景 2.5 分鐘持續播放（修正前約 60 秒後停住，見下） |
| 系統媒體鍵 | ⚠️ 這支手機上 Telecom 的 `HeadsetMediaButton` 一直是 global priority session，adb 送出的媒體鍵到不了 app；通知與鎖定畫面的按鈕待使用者實測 |

- **第二層選單：** 子選單開在父選單右側，左右都放不下時 floating-ui 退回右側而超出畫面。`src/plugins/in-app-menu/renderer/Panel.tsx` 加上 `shift({ crossAxis: true })`，放不下時推回畫面內、疊在父選單上（Electron 的窄視窗也適用）。
- **背景播放停住，三個原因**（GeckoView 的設定檔 `files/geckoview-config.yaml`，由 `GeckoPage` 在啟動時寫入）：
  1. Gecko 在 app 進入背景時把 utility 與 GPU 行程降為 IDLE 優先權，手機約 30 秒內就把它們結束，音訊或影片解碼器跟著消失，YTM 重設播放器。設定 `media.utility-process.enabled: false`、`layers.gpu-process.enabled: false`，解碼改在不會被結束的行程（播放音訊的內容行程、有 `PlaybackService` 的主行程）。
  2. renderer 讓影片經過 AudioContext 輸出，而 Gecko 的 autoplay 政策讓沒有使用者手勢時建立的 AudioContext 停在 suspended，**輸出是靜音**（app 自動續播時，第一次點擊前都沒有聲音）。HyperOS 的 PowerKeeper 會在背景 app 連續 60 秒輸出靜音後暫停它的音訊（logcat：`isLongTimeZeroData out of time 60S`、`pause zero data process`），播放就停在原地。設定 `media.autoplay.default: 0`（含 Web Audio），AudioContext 一建立就是 running。
  3. 平板（MIUI Pad）沒有遇到，推測是兩台的系統策略不同。
- 這三個設定也套用在平板上，平板尚未重測。

## GeckoView 可行性驗證（Android，2026-10-24）

為了在 Android 上登入 Google，測試把 Android WebView 換成 GeckoView（Firefox 的引擎）。驗證時用的是同一個 app 裡獨立的入口 `GeckoActivity`（第 2 階段已移除，GeckoView 改到 `MainActivity` 裡）；`assets/pear-gecko/` 是內建的 WebExtension：`probe.js` 以 `world: "MAIN"`、`document_start` 注入頁面主世界，`report.js` 經 native messaging 把結果回報給 app（logcat tag `PearGecko`）。

| 項目 | 結果（Xiaomi 平板，GeckoView 153） |
|---|---|
| Google 登入 | ✅ 使用者實測可以登入，關閉再開仍保持登入（app 回報 `signedIn: true`） |
| 在頁面腳本之前注入 | ✅ 注入時 `readyState` 是 `loading`、頁面還沒有任何 `<script>`、`ytcfg` 尚未定義 |
| 主世界的修改生效 | ✅ 包住 `JSON.parse` 看得到頁面的呼叫（do-not-track 的做法可用） |
| 桌面版網站 | ✅ UA 為桌面版 Firefox 153（Linux） |
| 體積 | ❌ debug APK 從 61.6 MB 變成 256 MB（arm64），minSdk 需要 26 |

- 用 GeckoView 153 而不是最新的 158：154 起依賴 Kotlin 2.4，這個專案的 Kotlin 2.2 讀不了。
- 改用 GeckoView 的完整計畫（IPC 橋接、bundle 與設定的注入、Tauri 的角色、體積）見 [GECKOVIEW-PLAN.md](GECKOVIEW-PLAN.md)。
- **第 1 階段（bundle 在 Gecko 裡跑）已完成：** 全部插件打開時，GeckoView 與 WebView 載入的插件相同（28 個）；找到並修正兩個 Firefox 才有的問題（`pear-trans` 元素建構失敗、AudioContext 被 autoplay 政策暫停而沒有聲音）。細節見計畫文件的「第 1 階段結果」。
- **第 2 階段（IPC 橋接）已完成：** Android 版現在預設用 GeckoView 顯示頁面，頁面與 Rust 之間走 native messaging（`inject/tauri.ts` → extension → `GeckoBridgePlugin` → `src/gecko_bridge.rs`）。平板上驗證了設定讀寫、歌曲資訊、選單（開啟、點擊、外掛開關即時生效）、`net_fetch`（歌詞）、Rust 在頁面執行腳本（重新整理）與設定變更後的重新載入。細節與延遲見計畫文件的「第 2 階段結果」。
- **第 3 階段（取代 WebView）已完成：** GeckoView 在背景繼續播放（`PlaybackService` 讓行程存活即可），系統媒體控制、連結（執行中與冷啟動）、從最近任務滑掉後重開都正常。在第一頁按返回鍵改成退到背景繼續播放（原本會結束行程）；「重新啟動」在 Android 上改由 Kotlin 重新開啟 app（Tauri 的 `restart()` 在 Android 上只會結束行程），導覽列選單也顯示了；「複製目前網址」改讀 GeckoView 的網址。記憶體約 1.25 GB（PSS），背景空白 WebView 的 renderer 只佔 5.9 MB。鎖定畫面上的媒體控制也已實機確認。分割畫面由寬變窄時播放頁專輯圖被壓扁（YTM 在版面切換中量測置中的 margin），`inject/shim.ts` 在 resize 結束後再送一次 `resize` 讓它重新量測。細節見計畫文件的「第 3 階段結果」。
- **第 4 階段（全面驗證）已完成：** 平板驗證表格以 GeckoView 重測都通過（見上方「GeckoView 版的平板驗證」），廣告阻擋、SponsorBlock、crossfade、歌詞都正常；修正「需要重新啟動」對話框在 Android 上只會結束 app 的問題。release APK 213 MB（壓縮後 99 MB），記憶體比 WebView 引擎多約 380 MB。

## 手動驗證清單

- [x] Google 登入成功（Windows，預設 UA）
- [x] 登入後正常關閉、重開仍保持登入（Windows）
- [ ] 確認單獨按 Alt 進入選單模式時，頁面本身是否也會凍結（若是，需攔截 `WM_SYSCOMMAND`/`SC_KEYMENU`）
- [ ] 通知實際顯示、Last.fm 登入流程、ListenBrainz 送出
- [x] Android 實機平板：播放、觸控操作、背景播放與鎖定畫面控制
- [x] Android 實機平板：Google 登入（UA／Client Hints 修改後重測：仍被拒）
- [ ] Android 實機平板：選單外觀與操作、在「預設開啟」加入 `music.youtube.com` 後點連結
- [ ] discord：實際 Discord 用戶端顯示 Rich Presence
- [ ] api-server：新 client 的 Allow/Deny 對話框、HTTPS
- [ ] taskbar-mediacontrol：縮圖按鈕外觀
- [x] notifications：互動式 toast 的三個按鈕實際按下、系統匣 trayControls
- [ ] notifications：按鈕的新文字與白色圖示在深色主題下的外觀；開發版 toast 上的「Pear Tauri PoC (dev)」名稱與圖示；安裝版的 toast 顯示 app 名稱
- [x] downloader：Android 存到「音樂/Pear Desktop」，MediaStore 讀得到標籤，檔案內有封面
- [x] downloader：Windows 的資料夾選擇對話框
- [ ] downloader：Android 音樂 app 裡的顯示；Premium 的 256 kbps 格式（需要 Premium 帳號，而且目前的工作階段未登入）
- [x] proxy：Android 的原生輸入對話框、extension 的 proxy（電腦上的 SOCKS5 測試伺服器 + `adb reverse`）
- [x] do-not-track With blocklists：Android 的阻擋
- [ ] 從頁面執行 `location.reload()` 會讓 Windows 版的頁面停住（停用這輪新增的請求過濾後仍然如此；瀏覽器層級的重新整理，也就是 CDP `Page.reload`，等同「檢視 › 重新整理」用的 WebView2 `Reload`，則正常），原因未查
