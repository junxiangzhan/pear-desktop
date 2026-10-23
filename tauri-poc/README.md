# Tauri v2 PoC

最小可行的 Tauri 外殼：載入 music.youtube.com，並把現有的 renderer（`src/renderer.ts` 與所有插件的 renderer 部分）原封不動注入頁面。目的是在投入完整移植前，先驗證登入、播放與 Android 可行性。

## 架構

```
tauri-poc/
├─ inject/
│  ├─ entry.ts         # 依序載入 shim → plugin-stubs → @/renderer
│  ├─ shim.ts          # 取代 preload.ts 透過 contextBridge 暴露的全域物件
│  └─ plugin-stubs.ts  # 在 shim 之後才載入 virtual:plugins（見下方「注意」）
├─ vite.inject.config.mts   # 把上述打包成單一 IIFE（丟棄抽出的 CSS，與 Electron 一致）
├─ placeholder/        # Tauri 需要 frontendDist；主視窗其實載入遠端網址
└─ src-tauri/
   ├─ src/lib.rs       # 視窗、loader、pear:// 協定、save_config 指令
   ├─ capabilities/ytm-remote.json  # 只允許遠端 YTM 呼叫 save_config
   └─ gen/android      # `tauri android init` 產生的 Android 專案
```

### 注入方式

1. 一段很小的 init script（所有 frame 都會執行，但只在 top frame 動作）用**同步 XHR** 向自訂協定 `pear` 取得 bundle（Windows/Android 是 `https://pear.localhost/renderer.js`，macOS/Linux 是 `pear://localhost/renderer.js`）。
2. 透過 Trusted Types policy 以間接 `eval` 執行。YTM 的 CSP 允許 `'unsafe-eval'`，但要求 `require-trusted-types-for 'script'`。
3. 這樣執行時機等同 Electron 的 preload：在任何頁面腳本之前、在主世界執行。

**為什麼不直接把 bundle 當 init script：** 實測 WebView2 收到這個 6.6 MB、多行的 bundle 時，browser process 會卡在 100% CPU，整個頁面和 DevTools 都無回應。同樣大小的單行字串則沒問題。

### 設定

renderer 會同步讀 `window.mainConfig.get()`，但 Tauri IPC 只有非同步，所以 Rust 啟動時把 `config.json` 接在 bundle 前面一起送出；寫入走 `save_config` 指令（遠端 origin 經 capability 授權）。

### 插件

有 backend 的 23 個插件在 shim 裡強制停用（`UNPORTED_BACKEND_PLUGINS`）。另外 `performance-improvement` 在 PoC 中**預設停用**（`POC_DEFAULT_OVERRIDES`，原因見下方「卡頓的原因」）；這只是預設值，在 `config.json` 寫入 `{"plugins":{"performance-improvement":{"enabled":true}}}` 仍可重新啟用。沒有 backend 的 `ipcRenderer.send/invoke` 只會印 debug log。

**CSS：** renderer 的 `import 'mdui/mdui.css'` 會被 Vite 抽成獨立 CSS 檔，但 Electron 的 `get-renderer-script` 只讀 `<script>`，所以這份 CSS 在 Electron 裡從未套用。PoC 一度把它內嵌注入，結果 mdui 的 `:root { background-color }` 在 YTM 半透明背景後面畫出灰色色塊；現在改為直接丟棄，行為與 Electron 一致。

**注意：** `shim.ts` 不能 import `virtual:plugins`。它會靜態載入所有插件，插件的頂層程式碼會在 shim 定義全域物件之前執行，然後丟 `Cannot read properties of undefined (reading 'on')`。

## 執行（Windows）

在 repo 根目錄：

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

測試時請正常關閉視窗。強制結束行程會讓 WebView2 來不及寫回 cookie，登入狀態會遺失。另外，單獨按一下 Alt 會讓視窗進入 Windows 的選單模式，期間 WebView2（至少 DevTools）會停止回應，按 Esc 即可離開。

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

因此 PoC 預設停用這個插件。停用後已確認：沒有 `<a-f>` 元素、`setTimeout` 恢復為原生函式、其他插件與登入狀態不受影響。同樣的開銷在 Electron 版也存在（見上表），是否要在 Electron 版也調整，值得另外評估。

## 手動驗證清單

- [x] Google 登入成功（Windows，預設 UA）
- [x] 登入後正常關閉、重開仍保持登入（Windows）
- [ ] 確認單獨按 Alt 進入選單模式時，頁面本身是否也會凍結（若是，需攔截 `WM_SYSCOMMAND`/`SC_KEYMENU`）
- [ ] 啟用其他純 renderer 插件：PoC 沒有選單，可編輯 `%APPDATA%\com.github.pear-devs.tauri-poc\config.json`，例如 `{"plugins":{"album-color-theme":{"enabled":true}}}`，然後重開
- [ ] Android 實機平板：登入、播放、觸控操作
