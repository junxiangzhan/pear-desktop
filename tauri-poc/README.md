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
```

```
tauri-poc/
├─ inject/                   # 頁面端（打包成單一 IIFE）
│  ├─ entry.ts               # 順序：shim → plugin-stubs → preload-plugins → menu-runtime → plugin-glue → @/renderer
│  ├─ shim.ts                # window.ipcRenderer / mainConfig / electronIs / __pearCrossOriginFetch
│  ├─ ipc.ts、tauri.ts        # 對 Rust 的 IPC（不依賴 @tauri-apps/api）
│  ├─ compat/                # build alias：electron、electron-is、custom-electron-prompt、@/config、prompt-options
│  ├─ plugin-stubs.ts        # 把插件預設設定送給 Rust（plugins_manifest）
│  ├─ preload-plugins.ts     # 在頁面裡跑插件的 preload 部分（沿用 src/loader/preload.ts）
│  ├─ menu-runtime.ts        # 執行插件 menu()、序列化給 Rust、處理點擊
│  └─ plugin-glue.ts         # 只有 UI 的 backend（captions / quality / music-together 提示框、PiP 按鍵）
├─ plugin-support.json       # 尚未移植的插件清單（Vite 與 Rust 共用）
├─ placeholder/prompt.html   # 提示框視窗（取代 custom-electron-prompt）
└─ src-tauri/
   ├─ src/                   # Rust 主行程（見上圖）
   ├─ capabilities/          # ytm-remote.json：遠端 origin 只能呼叫這些指令；prompt.json
   └─ gen/android            # `tauri android init` 產生的 Android 專案
```

`src/` 與 `vite-plugins/` 的配合修改（不影響 Electron 行為）：`plugin-loader` 可一次保留多個插件部分並排除指定插件；`virtual:plugins` 可輸出多個 `<mode>Plugins()`；新增 `src/providers/net.ts`（`crossOriginFetch`），synced-lyrics 的 LRCLib / Genius / Megalobiz / YTMusic 改用它；notifications 的純常數移到 `constants.ts`；scrobbler 選單不再 import backend。

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
| backend 已移植到 Rust | sponsorblock、synced-lyrics（`synced-lyrics:fetch`）、lumiastream、tuna-obs、notifications（基本模式）、picture-in-picture、transparent-player、scrobbler、shortcuts（全域／視窗內快捷鍵、媒體鍵）、precise-volume |
| backend 只有 UI，改在頁面處理 | captions-selector、quality-changer、music-together（`plugin-glue.ts`） |
| 尚未移植（強制停用、選單變灰） | amuse、api-server、auth-proxy-adapter、crossfade、discord、do-not-track、downloader、in-app-menu、taskbar-mediacontrol、touchbar |

- **crossfade：** 它的 backend 用 youtubei.js 解析串流網址，但 youtubei.js 17 需要自備 JavaScript 直譯器才能 decipher（Node 與瀏覽器版都只內建會丟例外的 `evaluate`），所以 Electron 版的 crossfade 很可能本來就無法運作，先不移植。
- **notifications：** 互動式 Windows toast（附播放按鈕）尚未移植；`interactive` 開啟時退回基本通知。
- **shortcuts：** Linux 的 MPRIS 尚未移植。

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

## 驗證結果（2026-10-23）

| 項目 | Windows（WebView2 157） | Android 模擬器（API 34, WebView 125） |
|---|---|---|
| 頁面載入、renderer 注入 | ✅ `data-os=Windows`、`player-api-loaded` | ✅ 桌面版 YTM、`data-os=Linux` |
| 插件載入 | ✅ navigation、performance-improvement、playback-speed | ✅（同一份 bundle） |
| 未登入播放 | ✅ 4 秒前進 4.01 秒 | ✅ 前景可播（含廣告） |
| 設定存取（遠端 IPC） | ✅ `config.json` 寫入並於重開後生效 | 未測 |
| 背景播放 | — | ❌ 按 Home 後立即暫停；行程被 Android cached-apps freezer 凍結（`isFrozen=true`），需要 foreground service |
| Google 登入 | ✅ 預設 UA 可登入；正常關閉後重開仍保持登入（完整的 SID/SAPISID/LOGIN_INFO cookie）。用 `taskkill /F` 強制結束會來不及寫回 cookie，登入狀態會遺失 | 未測 |
| 流暢度：模擬捲動（debug build，未登入） | 原始網站 vs 注入後幾乎相同：靜置 ~165 fps；捲動 ~138 fps、長任務 7 次（兩者皆是 YTM 自身載入內容）。同條件下 Electron 開發版的滾輪測試並沒有比較好 | 靜置 59.3 fps、p95 16.8 ms、無長任務 |
| 流暢度：使用者實際滾輪捲動（已登入，20 秒） | `performance-improvement` **開啟：550 個掉幀**、主執行緒忙碌 76%、`animationiteration` 事件 18,854 次。**關閉：0 個掉幀**、忙碌 22%、無 `animationiteration`。兩者 GC 都很少（Major GC 4–5 次、合計約 60–110 ms），卡頓不是 GC 造成的 | — |
| 同一插件在 Electron vs Tauri（模擬滾輪、插件開啟） | 兩者開銷相近：`animationiteration` 1,806 vs 2,009 次、長任務 3 vs 2 次。WebView2 並沒有放大這個問題 | — |
| 執行檔大小 | release exe 11 MB（含 6.6 MB bundle；壓縮後 3.6 MB），Electron 執行環境本身 355 MB | debug APK 134 MB（含除錯符號，非代表值） |

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

## 手動驗證清單

- [x] Google 登入成功（Windows，預設 UA）
- [x] 登入後正常關閉、重開仍保持登入（Windows）
- [ ] 確認單獨按 Alt 進入選單模式時，頁面本身是否也會凍結（若是，需攔截 `WM_SYSCOMMAND`/`SC_KEYMENU`）
- [ ] 通知實際顯示、Last.fm 登入流程、ListenBrainz 送出
- [ ] Android 實機平板：登入、播放、觸控操作
