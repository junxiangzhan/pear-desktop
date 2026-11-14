# 自己的介面＋YouTube 的播放器（不載入 YTM 介面）

目標：Pear 自己維護整個介面，不再讓 YTM 的介面（`music_polymer_inlined_html.js`，數 MB 的 Polymer 程式）啟動。資料直接呼叫 innertube（沿用 `src/mobile-ui/model/parse.ts`）；播放交給 YouTube 自己的播放器（`base.js`）。這樣可以省掉現行做法裡的這些部分：搬移 `<video>`、用隱藏的 YTM 元素代執行選單項目、轉交點擊、history 隔離、隱藏 YTM 介面、跟著 YTM 換播放列。

YTM 的介面程式和播放器是分開載入的兩支腳本：

- `https://music.youtube.com/s/<版本>/music_polymer_inlined_html.js`：YTM 的介面
- `https://music.youtube.com/s/player/<版本>/player_es6.vflset/<語言>/base.js`：YouTube 的播放器（YTM、YouTube、嵌入式播放器共用）

## 測試方法（2026-11-13，內建瀏覽器，Chromium 手機模式，未登入）

在 YTM 頁面裡建一個同源的 `about:blank` iframe（有 YTM 的 cookie，沒有 YTM 的介面），只放進：

1. `ytcfg`：完整複製頁面的 `ytcfg.data_`（只用首頁 HTML 裡的那一段不夠，播放器會讀到不存在的欄位）。
2. `base.js`：用 `WEB_PLAYER_CONTEXT_CONFIGS.WEB_PLAYER_CONTEXT_CONFIG_ID_MUSIC_WATCH.jsUrl` 載入（頁面有 Trusted Types，要先建 policy），再加上 `cssUrl`。

建立播放器，參數照 YTM 的呼叫方式：

```js
(yt.player.Application.createAlternate || yt.player.Application.create)(
  host,                                   // 放播放器的元素
  { args: {}, attrs: { id: 'player' } },
  ytcfg.get('WEB_PLAYER_CONTEXT_CONFIGS').WEB_PLAYER_CONTEXT_CONFIG_ID_MUSIC_WATCH,
  () => ({ signedOut: true }),            // getAppOauthToken：{ signedOut } 或 { token } 或 { pendingResult }
);
```

播放器元素（`.html5-video-player`）上就是平常 `#movie_player` 的 API（`loadVideoByPlayerVars`、`playVideo`、`getPlayerState`、`seekTo`、`addEventListener('onStateChange')`…），插件用的就是這套。

播放器**不會自己發 `/player` 請求**，要由宿主取得回應後交給它：

```js
player.loadVideoByPlayerVars({ raw_player_response: response });
```

YTM 自己的 `/player` 請求內容（請求本體以 gzip 壓縮）：

```json
{
  "videoId": "…",
  "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": "1.2026…", "platform": "MOBILE", … } },
  "playbackContext": {
    "contentPlaybackContext": { "html5Preference": "HTML5_PREF_WANTS", "signatureTimestamp": 20767, "referer": "…", "vis": 10, "lactMilliseconds": "…", "autoCaptionsDefaultOn": false, "mdxContext": {} },
    "devicePlaybackCapabilities": { "supportsVp9Encoding": true, "supportXhr": true }
  },
  "cpn": "…",
  "captionParams": {},
  "serviceIntegrityDimensions": { "poToken": "…" }
}
```

沒有 `serviceIntegrityDimensions.poToken` 時回應是 `UNPLAYABLE`（「必須重新載入頁面。」）。App 已經能自己產生 PO token（`src/inject/youtubei.ts`，bgutils-js）。

## 結果（第一輪，內建瀏覽器）

| 檢查 | 結果 |
|---|---|
| 不啟動 YTM 介面，單獨建立 YouTube 播放器 | ✅ 整個 iframe 只有 225 個 DOM 節點 |
| 播放器 API 與 `#movie_player` 相同 | ✅ |
| 播放自己取得的 `/player` 回應 | ✅ 開始播放（狀態 1、MSE `blob:`），標題、長度正確 |
| 繼續播放與拖曳進度 | ❌ 播到約 50 秒就停住。第二輪在平板上沒有這個問題，推測是內建瀏覽器窗格隱藏（背景頁）造成的 |

## 第二輪：平板（GeckoView），不載入 YTM 介面

做法（`src/inject/own-engine-spike.ts`，暫時的；localStorage `pear-own-engine` 設為 `1` 才作用）：

- 在 document start 用 MutationObserver 攔下 YTM 介面的腳本（`music_polymer_inlined_html.js`、`scheduler.js`、`webcomponents-*`、`custom-elements-es5-adapter.js`）：插入時改掉 `type` 並移除，它們就不會執行。Gecko 已經沒有 `beforescriptexecute`。另外讓 `customElements.define` 忽略 `ytmusic-`／`yt-`／`ytd-`／`tp-yt-` 開頭的元素作為保險。
- `base.js` 原本是 YTM 介面動態載入的，所以要自己載入（ytcfg 的 `jsUrl`、`cssUrl`，頁面有 Trusted Types，要建 policy）。
- 照上面的參數建立播放器，只給 `videoId`：`player.loadVideoByPlayerVars({ videoId })`。

| 檢查 | 結果 |
|---|---|
| YTM 介面沒有執行 | ✅ 4 支腳本被攔下，`ytmusic-app` 沒有定義，頁面 96 個節點；建立播放器後 315 個 |
| 播放器自己取得歌曲資料 | ✅ 不需要我們送 `/player`。播放器有自己的 BotGuard（request key 和 bgutils 相同），自己產生 PO token 並附在 `/player` 與串流請求（`pot`）上 |
| 整首播放 | ✅ 狀態 OK、SABR 串流；拖曳到 150 秒、205 秒都正常，播到結尾（狀態 0、`video.ended`） |
| 啟動時間 | 第一首約 30 秒才開始（狀態 -1、3、-1、3、1，像是重試；應該是第一次初始化 BotGuard）。之後的歌 0.3 秒就開始播。正式做法要在 App 啟動時先預熱 |
| 自己呼叫 innertube | ✅ 不靠 YTM，用 ytcfg 的 `INNERTUBE_CONTEXT` 加 `X-Goog-Visitor-Id` 等標頭呼叫 `/next`，拿到 50 首的電台清單（未登入） |
| 廣告 | 部分 ✅：9 次載入有 1 次出現 `ad-showing`（播放器自己的廣告系統有作用）。廣告是否完整播完再接回歌曲，因為之後沒再遇到廣告，還沒看到。這些歌的回應多半只有預測用的 `clientForecastingAdRenderer`（開頭）與 `adBreakServiceRenderer`（結尾），不是真正的廣告 |

## 待辦

1. 第一首的啟動延遲：查清楚那 30 秒在做什麼（BotGuard、重試），App 啟動時預熱。
2. 已登入：`getAppOauthToken` 該回傳什麼；自己呼叫 innertube 時的驗證標頭（`Authorization: SAPISIDHASH …`、`X-Goog-AuthUser`）。
3. 廣告：多遇幾次，確認廣告播完會接回歌曲、略過按鈕可用。
4. 播放紀錄：播放器會自己送 `/api/stats/playback`、`watchtime`，登入後確認「觀看記錄」有更新。
5. 背景播放（關螢幕）、通知列的媒體控制。
6. 換成正式架構：介面沿用 `src/mobile-ui/ui`、`model`，`engine/ytm.ts` 改成直接控制這個播放器並自己呼叫 innertube；待播清單由我們自己管理（YTM 的待播資料不存在了）。
7. 插件：只碰播放器 API 與 `<video>` 的插件應該可以直接用（API 相同）；會改 YTM 畫面的插件要改寫。
8. 備案：youtubei.js 解出串流網址，用自己的 `<audio>` 播放（crossfade 與下載器已經在用，`src/inject/crossfade.ts`）。

## 第三輪：正式搬移（分支 `feat/own-engine`）

新介面不再疊在 YTM 介面上，`src/mobile-ui/engine/` 改寫成：

| 模組 | 內容 |
|---|---|
| `boot.ts` | 在 document start 攔下 YTM 介面的腳本；載入 `base.js`，照 YTM 的方式建立播放器（不給 app OAuth token，播放器用頁面的 cookie）。播放器元素沿用 `movie_player` 這個 id，Pear 的 renderer 與插件照常找得到 |
| `innertube.ts` | 自己呼叫 innertube：ytcfg 的 `INNERTUBE_CONTEXT`、visitor id、client 名稱與版本；已登入時加上 `SAPISIDHASH`（含 1P／3P）與 `X-Goog-AuthUser` |
| `player.ts` | 播放器的盒子是 `<pear-mobile-ui>` 的子元素，指定到介面的 slot（廣告時是封面位置），沒有地方顯示時停在畫面外的 slot。播放狀態、時間、廣告狀態（`ad-showing`、略過鈕、廣告標題與贊助商）都從播放器讀。播放器自己的控制列藏起來，只留廣告介面；略過鈕仍由介面的 `<label>` 轉送點擊 |
| `queue.ts` | 待播清單由介面自己管理：`/next` 建立（歌曲、清單、電台），續載標記，播完後接 YTM 的 automix；「接著播放」「加入待播清單」用 `/music/get_queue`；移除、拖曳、隨機、重複播放、關閉待播清單都在本地。讚的狀態讀 `/next` 的 `playerOverlays` |
| `commands.ts` | 選單項目與播放按鈕的 endpoint：watch、queueAdd、removeFromQueue、dismissQueue、like、feedback、subscribe／unsubscribe、share（`/share/get_share_panel` 取得連結，系統分享或複製）、未登入時的 modal（導向登入）、url |
| `library.ts` | 評分、播放清單、帳號選單、Pear 設定選單 |
| `index.ts` | 對介面的出口；把 `__pearPlayerControls`（下一首、上一首、隨機、重複、讚）交給 `src/renderer.ts`，Rust 送來的指令（通知列按鈕）改由它執行 |

拿掉的東西：搬移 `<video>`、隱藏 YTM 版面、用隱藏的 YTM 元素代執行選單、`press()`、跟著 YTM 換播放列、讀 YTM 的待播資料。

### 驗證（平板，未登入，真實觸控）

| 檢查 | 結果 |
|---|---|
| YTM 介面沒有啟動，首頁由自己的 API 呼叫載入 | ✅ 整頁 318 個 DOM 節點（原本約 4,400） |
| 點歌播放 | ✅ `/next` 建立 50 首待播清單，播放器自己送 `/player` 並開始播放 |
| 下一首、上一首（3 秒內回到開頭） | ✅ |
| 待播清單拖曳排序 | ✅ |
| 從待播清單中移除（選單） | ✅ 列消失，顯示提示 |
| 歌詞、相關內容分頁 | ✅ |
| 所有 innertube 請求 | ✅ 都是 200，沒有錯誤 |

### 還沒做或還沒驗證

- 帳號選單裡原本是 YTM 對話框的項目（切換帳戶、登出、設定、上傳歌曲、說明、提供意見）：目前顯示「這個版本還不支援」。
- 冷啟動第一首的延遲（播放器初始化 BotGuard）：這一版還沒量，也還沒預熱。
- MV 的影片模式。
- 通知列的媒體控制（經由 renderer 的 `__pearPlayerControls`）、關螢幕背景播放。
- 插件：renderer 找得到 `#movie_player`，只碰播放器的插件應該可用；會改 YTM 畫面的插件不會有作用。`peard:get-queue` 等讀 YTM 待播資料的 IPC 還沒改。
- App 重新開啟時（網址停在 `/watch`）不會接著播上次的歌。
- 搜尋結果、專輯／播放清單頁的播放、隨機播放、電台按鈕：走同一條 `runEndpoint`，這一版還沒逐一點過。

### 已登入（平板，使用者登入測試版之後）

| 檢查 | 結果 |
|---|---|
| 驗證標頭 | ✅ `SAPISID`、`__Secure-1PAPISID`、`__Secure-3PAPISID` 都讀得到；自己的請求被當成已登入：首頁有「再聽一次」，頭像、帳號選單、媒體庫（使用者自己的播放清單）、播放記錄都正常 |
| 已登入播放 | ✅ 在「唱歌」播放清單播放第一首，播放器的 `/player` 回應 OK |
| 觀看記錄 | ✅ 播放約 40 秒後，`FEmusic_history` 最上面多了這首歌（播放器自己回報） |
| 廣告 | ✅ 這次遇到可略過的影片廣告：播放列變成黃色、封面位置播放廣告影片、標題換成廣告名稱並加上「廣告」標記、選單鈕換成「略過廣告」。真實觸控按「略過廣告」：廣告結束、歌曲開始播放、播放器回到畫面外 |
| 啟動延遲 | 這次從點歌到廣告開始不到 1 秒，沒有遇到之前約 30 秒的冷啟動 |
| 待播清單續載 | ✅ 436 首的清單：50 → 197 首（捲到底三次） |
| 讚的狀態 | ✅ 播放器頁的讚／倒讚按鈕跟著 `/next` 的狀態（沒有按） |

沒有測（會動到使用者的帳號）：按讚、訂閱、加入或移除播放清單、登出、切換帳戶。

## 插件

逐一讀過每個插件在頁面這一側用到什麼（2026-11-13）。在這個架構下，頁面上**沒有** YTM 介面的任何元素（`ytmusic-*`、`#queue` 與它的 Redux store、`networkManager`、`#like-button-renderer`、YTM 的彈出視窗與提示）；**有**的是 `#movie_player`（YouTube 的播放器與完整 API）、它的 `<video>`、`src/renderer.ts`（照常呼叫插件的 `onPlayerApiReady`）。

在平板上實際啟用「指數化音量調整」（exponential-volume）後重新載入：它把 `HTMLMediaElement.prototype.volume` 的 setter 換成自己的（`set(e){let t=e**3;…}`），插件的載入流程在新架構下照常運作。測完已經關回去。

| 插件 | 結論 | 原因與需要的修改 |
|---|---|---|
| equalizer、audio-compressor、skip-silences、exponential-volume、sponsorblock、custom-output-device | 可以直接用 | 只用播放器 API、`<video>`、AudioContext。custom-output-device 另外要看 GeckoView 支不支援 `setSinkId` |
| disable-autoplay、do-not-track、performance-improvement | 可以直接用 | 前者靠播放器的 `videodatachange`；do-not-track 的外觀過濾只作用在 document，碰不到新介面（無害） |
| notifications、scrobbler | 大致可用 | 後端在 Rust；但歌曲資訊（見下方「歌曲資訊」）的播放時間不會更新，可能缺專輯名稱 |
| quality-changer | 部分 | 切換畫質的 API 可用；按鈕放在 YTM 的播放器上（`.top-row-buttons.ytmusic-player`），要移到新介面 |
| playback-speed | 部分 | 速度設定作用在 `<video>`，可用；滑桿在 YTM 的選單裡，要移到新介面 |
| precise-volume | 部分 | 還原音量、IPC 調整可用；滑桿、滾輪、音量提示都掛在 YTM 的播放列與導覽列，要移到新介面 |
| downloader | 部分 | 下載本身在 Rust，可用；入口是 YTM 選單裡的項目，要加到新介面的歌曲選單；`location.href` 的備援不再對應目前的歌 |
| album-color-theme | 部分 | 取色可用（從播放器回應的縮圖）；樣式全部針對 YTM 版面。CSS 變數會繼承進 shadow root，新介面可以直接用 `--ytmusic-album-color` |
| visualizer | 部分 | 音訊部分可用；畫布要放在 `ytmusic-player#player` 裡，找不到就不畫，要在新介面留位置 |
| crossfade | 壞掉 | 只從網址導覽（`window.navigation` 的 `navigate`、`?v=`）偵測換歌，新架構播歌不改網址；用 `api.nextVideo()` 換下一首，播放器自己沒有待播清單。要改成聽 `videodatachange`、用新引擎的下一首，最好讓引擎事先告訴它下一首是什麼 |
| captions-selector | 壞掉 | 按鈕用 `!` 掛在 `.right-controls-buttons`（找不到就丟出例外，見下方第 1 點），提示用 YTM 的 toastService；字幕畫在播放器裡，而播放器平常在畫面外 |
| skip-disliked-songs | 壞掉 | 等 `#like-button-renderer`（永遠不會出現），按 YTM 的下一首。要改讀新引擎的 `likeStatus`、用新引擎的下一首 |
| synced-lyrics | 壞掉 | 歌詞來源（LRCLib、Genius）可用；但它把歌詞畫進 YTM 的歌詞分頁，YTMusic 來源用 `networkManager`。要在新介面的歌詞分頁提供掛載點，YTMusic 來源改用新引擎的 innertube |
| ambient-mode、video-toggle | 壞掉 | 都依賴 YTM 的播放器頁版面；在新介面要做成功能（影片模式要讓播放器在廣告以外也顯示） |
| album-actions | 壞掉 | 整個依賴 YTM 的專輯頁；要做成新介面專輯／播放清單頁的動作 |
| music-together | 壞掉 | 直接改寫 YTM 待播清單的 Redux store；要改成對新引擎的待播清單 API |
| unobtrusive-player | 壞掉 | 依賴 YTM 的 `player-ui-state`；可以變成新介面的設定（點歌時不打開播放頁） |
| blur-nav-bar、clock、navigation | 沒有作用 | 都掛在 YTM 的導覽列；在 Android 可以停用，或改成新介面的選項 |
| in-app-menu | 已取代 | 標題列在 Android 已隱藏，選單在帳號 sheet 裡；只需要它的選單資料 |
| auth-proxy-adapter、discord、picture-in-picture、shortcuts、transparent-player | 本來就在 Android 停用 | |
| taskbar-mediacontrol | 只有 Windows | |

### 共通問題（修了才能讓上面的插件可靠運作）

1. **一個插件丟出例外，後面的插件全部不會啟動**：`src/renderer.ts` 依序 `await` 每個插件的 `onPlayerApiReady`，沒有 try/catch。例如 captions-selector 一啟用，排在它後面的 equalizer、audio-compressor、visualizer、skip-silences 都拿不到播放器。要像 `startPlugin` 一樣個別包起來。
2. **歌曲資訊（`src/providers/song-info-front.ts`）只剩一半**：換歌（`videodatachange`）、播放／暫停、跳轉、音量可用；播放時間讀 YTM 的 `#progress-bar`，不會更新（影響 scrobbler、通知）；重複播放與自動播放的監聽對不存在的元素呼叫 `observe(null!)`，會丟出例外；讚的狀態送不出去；專輯與在地化標題來自 `getWatchNextResponse()`，可能是空的。要讓新引擎把時間、重複、讚、隨機、專輯與標題送進同樣的 IPC。
3. **待播清單與搜尋的 IPC 失效**：`peard:get-queue`、`add-to-queue`、`move-in-queue`、`remove-from-queue`、`set-queue-index`、`clear-queue` 都操作 YTM 的 `#queue`；`peard:search` 用 `networkManager` 與 `ytmusic-search-box`。要改接新引擎（像 `__pearPlayerControls` 一樣交給 renderer）。
4. **插件的 CSS 進不了新介面**：插件的樣式只套在 document，碰不到 `<pear-mobile-ui>` 的 shadow root（只有 CSS 變數會繼承）。要把插件的介面搬進新介面，需要掛載點（slot），或「把樣式也套進 shadow root」的機制。

## 第四輪：使用者回報與插件分流

| 回報 | 原因與處理 | 驗證（平板，已登入，真實觸控） |
|---|---|---|
| 讚、倒讚按了之後，切到別首再切回來不會保留 | 按讚本身有成功（`/like/like` 回 200、已登入）；是讀回來的狀態錯：YouTube 把讚的狀態搬到 view model（`playerOverlays…segmentedLikeDislikeButtonViewModel…likeStatusEntity.likeStatus`），引擎只找舊的 `likeButtonRenderer`，永遠讀成「沒按」。改成先找 `likeStatusEntity`，舊的當備援 | 按讚 → 伺服器 `LIKE`；下一首再回來，按鈕仍是按下的；再按一次取消 → `INDIFFERENT` ✅ |
| YTM 在播放頁按倒讚會自動換下一首（不移出待播清單） | 照做：倒讚成功後換下一首 | 按倒讚 → 換到下一首、伺服器 `DISLIKE`、那首仍在待播清單 ✅；測完已取消倒讚 |
| 找不到訂閱藝人的按鈕 | 藝人頁標題區讀 `subscriptionButton.subscribeButtonRenderer`（頻道 id、是否已訂閱、訂閱數、按鈕文字、訂閱／取消訂閱的 endpoint），加上「訂閱」按鈕 | 按「訂閱」→ 回應 `updateSubscribeButtonAction {subscribed: true}`；再按取消 → `subscribed: false` ✅。YouTube 的藝人頁在訂閱後一段時間內仍回報「未訂閱」，所以引擎記住這次執行期間的變更，重新打開頁面時以它為準 |
| 播放頁的藝人名稱要能點 | 待播項目從副標（「藝人 • 專輯 • 年份」）取藝人頁的連結，播放頁的藝人名稱點了打開藝人頁（播放頁先關掉） | ✅ 點「en」→ 播放頁收起、打開藝人頁 |

測試留下的紀錄：觀看記錄多了「天后」「拥过你的风」；讚、倒讚、訂閱都已取消。

### 插件分流：原介面與新介面

`plugin-support.json` 新增 `unavailableInMobileUi`：新介面執行時（Android、檢視 > Pear 行動版介面）這些插件視為不可用（和 `unavailable`、`unavailableOnMobile` 一樣強制關閉、選單裡變灰），改用 YTM 原介面時照常可用。Rust 的 `Config::unavailable()`（`src-tauri/src/config.rs`）在新介面開啟時把這份清單加進去，頁面的 `__PEAR_UNAVAILABLE_PLUGINS__`、`plugin_config`、啟用／卸載的監聽都改用它；切換新介面本來就會重新載入頁面，清單跟著更新。

列進清單的（16 個）：需要 YTM 介面才能發揮作用、沒有作用，或會影響其他插件的。

| 插件 | 原因 |
|---|---|
| album-actions | 整個依賴 YTM 的專輯頁 |
| album-color-theme | 樣式全部針對 YTM 版面，新介面看不到效果 |
| ambient-mode、video-toggle | 依賴 YTM 的播放器頁版面 |
| blur-nav-bar、clock、navigation | 掛在 YTM 的導覽列 |
| captions-selector | 掛按鈕時丟出例外；字幕畫在平常不顯示的播放器裡 |
| crossfade | 靠網址變化偵測換歌、用沒有待播清單的 `nextVideo()` 換歌，會打亂播放 |
| downloader | 唯一的入口是 YTM 選單裡的項目 |
| music-together | 直接改寫 YTM 待播清單的 Redux store |
| quality-changer、visualizer | 按鈕或畫布要放在 YTM 的播放器上，沒有地方就沒有作用 |
| skip-disliked-songs | 等 YTM 的讚按鈕（不會出現） |
| synced-lyrics | 把歌詞畫進 YTM 的歌詞分頁 |
| unobtrusive-player | 依賴 YTM 的 `player-ui-state` |

平板上確認：外掛功能選單有 21 個變灰（5 個桌面限定 ＋ 16 個新介面不可用）。

留著的：equalizer、audio-compressor、skip-silences、exponential-volume、sponsorblock、custom-output-device、disable-autoplay、do-not-track、performance-improvement、notifications、scrobbler、in-app-menu（新介面用它的選單資料）、playback-speed、precise-volume（功能本身可用，介面部分之後搬進新介面）。之後把某個插件移植到新介面，就把它從清單拿掉。

### 共通問題的修正

1. `src/renderer.ts`：每個插件的 `onPlayerApiReady` 各自 try/catch，一個失敗不會讓後面的插件拿不到播放器。
2. `src/providers/song-info-front.ts`（新介面時改從引擎取得，原介面行為不變）：
   - 播放時間：沒有 YTM 的進度條時，改從 `<video>` 的 `timeupdate` 取整秒送 `peard:time-changed`；
   - 重複播放、讚的狀態：讀引擎的 `repeatMode()`、`likeStatus()`，變更時通知（不再對不存在的元素 `observe(null!)`）；
   - 自動播放：沒有開關就不監聽；
   - 標題與專輯：從引擎的待播項目取 YTM 顯示的標題與專輯名稱。
3. 引擎交給頁面的介面整理成 `src/types/player-controls.ts`（`PlayerControls`、`playerControls()`），renderer 與歌曲資訊共用。
4. 待播清單與搜尋的 IPC（`peard:get-queue` 等、`peard:search`）：Rust 端已經沒有呼叫者（原本給 api-server 用，已移除），暫不移植。
5. 插件的 CSS 進不了新介面：等到第一個要把介面搬進新介面的插件時再設計掛載點。

## 第五輪：新介面主動相容插件（先做版面影響小的）

原則：插件把功能放進 YTM 介面（選單、播放列、播放器頁），新介面不載入那些地方，所以由新介面在自己的位置提供同樣的功能（插件啟用時才出現），功能本身仍用插件的邏輯。新介面這一側集中在 `src/mobile-ui/ui/plugins.ts`：用 `mainConfig.plugins.getPlugins()` 與 `plugin:enable`／`plugin:unload` 追蹤哪些插件開著（會即時反映選單上的切換），並提供各插件在選單裡的項目。

| 插件 | 新介面的做法 | 插件本身的修改 | 驗證（平板，真實觸控） |
|---|---|---|---|
| album-color-theme | 插件照常把歌曲顏色寫在 `<html>` 的 `--ytmusic-album-color`、`-dark`、`-ratio`（CSS 變數會繼承進 shadow root）；新介面開著這個插件時，用插件自己的比例把顏色混進 `--bg`、`--surface`、`--surface-high` | 無 | 播放「天后」：變數 `94, 93, 93`、比例 50%，介面背景跟著變 ✅ |
| downloader | 歌曲、專輯、播放清單的選單加「下載」，呼叫插件後端的 `download-song`／`download-playlist-request`（和它在 YTM 選單裡的按鈕相同）；`downloader-feedback` 的進度顯示成提示 | 掛 YTM 選單的地方先確認 `ytmusic-popup-container` 存在（原本用 `!`，在新介面會丟出例外） | 按「下載」：進度 0% → 42% → 83% → 100%，檔案在 `/sdcard/Music/Pear Desktop/` ✅（檔名用播放器回應的英文標題，是下載器原本的行為） |
| playback-speed | 播放中歌曲的選單加「播放速度」，子選單 0.5x～2x，目前的速度打勾 | 插件自己保存速度、別人改了會改回去，所以加上 `pear:set-playback-speed` 事件讓新介面請它改（`onUnload` 時移除） | 選 1.5x → `playbackRate` 1.5 ✅ |
| quality-changer | 播放中歌曲的選單加「畫質」，子選單列出播放器的 `getAvailableQualityLabels()`，選了就 `setPlaybackQualityRange`（插件原本的做法；不用它的原生對話框） | 無 | 子選單出現；這首是音訊，只有「自動」（MV 才有多種畫質）✅ |
| skip-disliked-songs | — | 新介面時改聽引擎的讚狀態（`playerControls().onLikeChange`），遇到倒讚就用引擎換下一首；引擎準備好時會發出 `pear:player-controls`。新介面自己按倒讚也會換下一首，所以介面只在那首還在播時才換，不會連跳兩首 | 在播放頁按倒讚：只跳一首 ✅；用 API 對一首歌按倒讚後回到那首：插件自動跳過 ✅ |

選單項目放在原本的項目後面；「播放速度」「畫質」只出現在播放中歌曲的選單（播放頁與播放列的 ⋮）。

`plugin-support.json` 的 `unavailableInMobileUi` 拿掉這 4 個（album-color-theme、downloader、quality-changer、skip-disliked-songs），剩 12 個。

測試時暫時啟用了這 5 個插件，測完已關閉（只剩 in-app-menu，和原本一樣）；倒讚都已取消。

剩下要相容的（版面影響較大）：synced-lyrics（歌詞分頁）、captions-selector（字幕要顯示在播放器以外的地方或讓播放器顯示）、visualizer（要留畫布位置）、ambient-mode、video-toggle（影片模式）、album-actions（專輯頁動作）、clock、blur-nav-bar、navigation、unobtrusive-player、crossfade（換歌偵測改聽 `videodatachange`、下一首由引擎提供）、music-together（要有引擎的待播清單 API）。

## 第六輪：重新啟動、歌詞、廣告略過

### 插件的「需要重新啟動」

`restartNeeded` 是每個插件自己宣告的，原因在插件掛的底層，和新介面無關：在播放器啟動時接上音訊圖（crossfade、skip-silences）、改寫媒體元素的原型（exponential-volume）、有 Rust 後端（downloader、notifications、scrobbler）、在頁面開始前執行（in-app-menu、performance-improvement）等。標成不需要的插件在新介面也是即時開關（例如 album-color-theme 一啟用顏色就套上）。之前測試時重新載入頁面只是保險。

新介面下插件分三類：

| 類別 | 插件 |
|---|---|
| 不需要新介面配合，直接可用 | equalizer、audio-compressor、skip-silences、exponential-volume、sponsorblock、custom-output-device、disable-autoplay、do-not-track、performance-improvement、notifications、scrobbler、in-app-menu（新介面用它的選單資料）、precise-volume（音量還原與 IPC；介面部分還沒搬） |
| 新介面主動相容（插件開著時，新介面在自己的位置提供它的功能） | album-color-theme、downloader、playback-speed、quality-changer、skip-disliked-songs、synced-lyrics |
| 還不能用（`unavailableInMobileUi`，新介面時變灰） | album-actions、ambient-mode、blur-nav-bar、captions-selector、clock、crossfade、music-together、navigation、unobtrusive-player、video-toggle、visualizer |

### 歌詞：synced-lyrics 放進播放頁的歌詞分頁

做法（之後的視覺類插件都用同一套）：新介面給插件一個「掛載點」：`mobileUiMount(slot, id)`（`src/types/player-controls.ts`）在 `<pear-mobile-ui>` 底下建一個元素並指定 slot，插件照它在 YTM 裡的方式把自己的元件畫進去；它是頁面樹的一部分，所以插件自己的 CSS 照常套用，畫面呈現不變，只是位置換到新介面的 slot（歌詞分頁開著插件時顯示 `<slot name="synced-lyrics">`，固定高度，插件的虛擬列表自己捲動）。

插件的修改：
- 偵測到新介面時掛到這個掛載點，不再等 YTM 的歌詞分頁（`renderer/utils.tsx` 的 `mountInMobileUi`、`renderer/index.ts`）。
- `LyricsRenderer` 原本用 `!` 取 YTM 的分頁元素來掛滑鼠事件，在新介面會丟出例外、整個不顯示；改成找不到時用自己的容器。
- YTMusic 歌詞來源原本用 YTM 的 `networkManager`，新介面時改用引擎的 innertube（`playerControls().innertube`）。

新介面另外提供 `<yt-formatted-string>` 的替代元素（`engine/boot.ts`）：插件用它顯示文字（`text.runs`／`simpleText`），沒有 YTM 介面時它是未知元素、什麼都不顯示（歌詞行高度是 0 就是這個原因）。只做插件用到的部分：把文字放進去。

驗證（平板）：歌詞分頁顯示插件自己的畫面：來源選擇（LRCLib）、大字歌詞與拼音 ✅；換歌時跟著載入下一首的歌詞 ✅。

### 廣告：播放頁直接用 YouTube 自己的略過按鈕

播放器本來就是 YouTube 的，播放頁的影片區顯示它完整的廣告介面（略過按鈕、第幾則廣告、廣告主連結），所以拿掉新介面在播放頁疊的「略過廣告」與隱藏 YouTube 略過按鈕的 CSS。播放列（迷你播放器）的影片只有封面大小，YouTube 的按鈕在那裡按不到，仍保留新介面的「略過廣告」（指向 YouTube 按鈕的 `<label>`）。

驗證（平板）：播放頁的影片上出現 YouTube 的「略過」、「第 1 則贊助商廣告（共 2 則）」與廣告主連結；真實觸控按「略過」→ 廣告結束、歌曲開始 ✅。

### 視覺類插件的方向

和歌詞相同：新介面提供掛載點，插件把原本的畫面畫進去，呈現不變、位置換到新介面。
- visualizer：只需要一個放畫布的容器（原本是 `#player`）→ 播放頁封面上的掛載點。
- ambient-mode：需要封面圖片與影片的位置 → 新介面提供封面的掛載點（含圖片）。
- video-toggle：需要在廣告以外也把播放器顯示在封面位置 → 引擎的影片模式。

## 第七輪：視覺類插件（visualizer、ambient-mode、video-toggle）

播放頁的封面改成一個框（`player.tsx` 的 `Cover`）：裡面是歌曲圖片，或播放器的影片（廣告、影片模式下的 MV），比例跟著內容（圖片 1:1、影片用影片本身的比例）。插件畫在上面的方式和在 YTM 的播放器上一樣，只是位置換成這個框的圖層（掛載點以 slot 放進來，插件開著時才有）：

| 圖層（由下而上） | slot／掛載點 id | 插件 |
|---|---|---|
| 封面後面 | `ambient-mode`／`#pear-ambient-mode` | ambient-mode 的模糊光暈 |
| 圖片或影片 | （新介面自己的）`video-player` | — |
| 封面上（廣告時不顯示，免得蓋住 YouTube 的略過按鈕） | `visualizer`／`#pear-visualizer` | visualizer 的畫布 |
| 最上層 | `video-toggle`／`#pear-video-toggle` | video-toggle 的「歌曲／影片」開關 |

掛載點只在播放頁開著時才被繪製（關著時 slot 不存在，元素沒有方框），插件可以用 `getClientRects()` 判斷要不要工作。

引擎（`engine/player.ts`、`engine/index.ts`）：
- 影片模式（`setVideoMode`）：開著且這首有影片（播放器回應的 `musicVideoType` 不是 `MUSIC_VIDEO_TYPE_ATV`）時，封面顯示影片。播放器放到哪個 slot 改成「畫面上最上層的影片位置」：播放頁的影片位置只在廣告或影片模式時存在，迷你播放器的只在廣告時存在；都沒有就停在畫面外。
- `PlayerControls` 新增 `cover()`／`onCoverChange()`（封面顯示的圖片網址、是否是影片，廣告也算影片）與 `setVideoMode()`；`withPlayerControls(callback)` 在引擎準備好時呼叫（只在新介面發生，那時介面的宿主已在頁面上）。

插件的修改：
- visualizer：容器先找新介面的掛載點，沒有才找 YTM 的 `#player`。`captureStream` 在 Gecko 沒有（只有 vudio 用到串流），沒有時給空的 `MediaStream`，不讓整個插件丟出例外。
- ambient-mode：把「放在哪裡、用哪張圖」改成參數；新介面時由 `cover()` 決定畫圖片或影片，掛到 `#pear-ambient-mode`，每秒與封面變化時檢查（播放頁關閉時移除），CSS 選擇器加上這個掛載點，樣式本身不變。
- video-toggle：新介面時開關放進 `#pear-video-toggle`（用插件自己的元件與 CSS、對齊設定），切換時呼叫引擎的影片模式並照常儲存 `hideVideo`；沒有影片的歌（ATV）隱藏開關，但不改使用者的選擇（YTM 版會把 `hideVideo` 改成 true）。新介面沒有 YTM 的原生切換，「原生樣式」也用插件的開關；「停用」不顯示開關、照設定顯示影片或圖片；「強制移除整個影片頁面」→ 只顯示圖片。插件關閉時（`stop`）封面回到圖片。

`unavailableInMobileUi` 拿掉這 3 個，剩 8 個：album-actions、blur-nav-bar、captions-selector、clock、crossfade、music-together、navigation、unobtrusive-player。

順便修正：synced-lyrics 的 YTMusic 歌詞來源判斷新介面用 `!app`，但頁面 HTML 裡有未升級的 `<ytmusic-app>`，所以會去呼叫不存在的 `networkManager`（記錄裡的 `t.networkManager is undefined`）；改成判斷 `networkManager` 是否存在。

### 驗證（平板，真實觸控）

- visualizer：畫布在封面上、大小等於封面（408×408），wave 類型正常跟著音樂畫 ✅。預設的 butterchurn 需要 WebGL，這台 GeckoView 完全沒有 WebGL（`webgl`、`webgl2` 都取不到），所以畫不出來（記錄：`Failed to create WebGL context: WebGL disabled`）；這是引擎限制，和新介面無關，舊介面在同一台也一樣。
- ambient-mode：圖片模式在封面後面有模糊光暈，換歌時跟著換、影片模式切回圖片時也跟著換 ✅；關閉播放頁時移除 ✅。影片模式的光暈畫不出來：這台 GeckoView 把影片畫進 canvas 得到全透明（`drawImage(video)` 不報錯但沒有像素，應是硬體解碼的影片無法讀回），同樣是引擎限制。
- video-toggle：ATV 的歌不顯示開關 ✅；MV（「為你痴狂」）封面顯示影片、開關在左上 ✅；真實觸控按開關 → 回到圖片、播放器停到畫面外、繼續播放、`hideVideo` 存成 true ✅；再按 → 回到影片 ✅；關閉播放頁 → 播放器停到畫面外、繼續播放 ✅；關閉插件 → 封面回到圖片 ✅。
- 沒有廣告可以測播放頁的影片位置與插件圖層的互動（廣告時 visualizer 與開關不顯示）。

測試時暫時啟用了這 3 個插件、把 visualizer 改成 wave，測完已關閉並把設定還原成 `{ enabled: false }`（測試版原本開著的 in-app-menu、synced-lyrics 不動）；`hideVideo` 回到 false。

## 第八輪：使用者回報（歌詞留白、插件選項、結尾廣告、開啟播放頁）

| 回報 | 原因 | 修正 | 驗證（平板） |
|---|---|---|---|
| 同步歌詞的面板下方有大塊留白 | 歌詞框固定高度 `min(70vh, 720px)`，寬版版面的面板比它高 | 寬版時歌詞框填滿面板（面板內容改成直向 flex，歌詞框 `flex: 1`）；窄版仍是固定高度、跟著頁面捲動 | 歌詞框與面板內容同高（654px），沒有留白 ✅ |
| 插件啟用後，要關掉清單再打開才能進到它的設定 | 按下後立刻重讀選單，但 Rust 是之後才重建選單（啟用的插件變成有選項的子選單），完成時才送 `refresh-in-app-menu` | 引擎記錄 `refresh-in-app-menu` 的次數（`appMenuVersion`），選單在每次重建後重讀；正開著的子選單消失時（插件關閉）退回還存在的最深一層 | 在「外掛功能」按「視覺化效果」：不必重開，那一列立刻變成子選單，裡面有「已啟用」「視覺化效果樣式」✅；在子選單裡取消「已啟用」→ 退回「外掛功能」，那一列變回勾選項 ✅ |
| 歌曲結尾有廣告時，廣告播完或略過後不會換下一首 | 歌曲結束只在「不是廣告時收到 ENDED」才算；結尾廣告時 ENDED 是在廣告期間送出，廣告結束後不再送 | 記下歌曲本身（非廣告）的播放位置；廣告結束（播完或略過）時，若歌曲已在結尾（最後 1.5 秒內）或播放器狀態是 ENDED，就算歌曲結束。每次載入只算一次，不會連跳兩首 | 沒有真的結尾廣告可測，用模擬：先標成廣告中再讓歌曲播完，狀態記錄是 `3ad, 1ad, 0ad`（ENDED 只在廣告中出現）；拿掉標記後換到下一首（`-1, 3, 1`）✅ |
| 播放清單或單曲時直接開啟播放介面 | — | 點清單裡的歌、卡片，以及專輯／播放清單頁的「播放」「隨機播放」「電台」都會開啟播放頁（`playAndOpen`）；待播清單裡的歌、選單裡的「接著播放」「加入待播清單」不變 | 點播放清單裡的歌 → 播放頁開啟 ✅；按播放清單頁的「播放」→ 播放頁開啟、從第一首開始 ✅ |

## 第九輪：使用者回報（播放列略過廣告、歌詞頂端、藝人頁、載入提示）

| 回報 | 原因 | 修正 | 驗證（平板） |
|---|---|---|---|
| 在播放列按「略過廣告」會打開播放頁 | 「略過廣告」的 `<label>` 把點擊轉給 YouTube 的略過按鈕；那個按鈕在播放器裡，播放器透過 slot 顯示在播放列中，所以這個點擊會從 slot 冒泡到播放列，觸發「開啟播放頁」 | 播放列的點擊若來自播放器（`#pear-player-host` 裡面）或略過標籤就不開啟播放頁（`player.tsx` 的 `fromPlayer`） | 模擬廣告（播放器移到播放列）後對播放器內的元素發出點擊 → 播放頁沒有開啟 ✅；點播放列的歌名 → 照常開啟 ✅ |
| 同步歌詞的最上方被擋住 | 歌詞來源選擇列（YTMusic ‹ ›）是黏在頂端的，歌詞捲動後應該滑走，除非滑鼠在它附近。兩個原因讓它一直擋著：插件把滑鼠位置的初始值設成 0（算是「在頂端」），觸控螢幕沒有滑鼠移動就一直是 0；另外新介面的掛載是先 render 再設成可見，插件掛上時找不到歌詞列表，捲動監聽沒有掛上 | 滑鼠位置初始值改成「沒有指標」（`Infinity`），點一下靠近頂端仍會叫出（觸控也會送出滑鼠移動）；新介面的掛載改成和 YTM 版相同的順序：先設成可見再 render | 修正前：捲動 264px 時選擇列仍在頂端（`top: 0`）；修正後：一開始（捲動 0）顯示，歌詞捲動 112px 後滑走（`top: -98px`），頂端的歌詞看得到 ✅（點一下頂端叫出它沒有測） |
| 藝人頁面的照片可以用漸層處理 | — | 藝人頁的橫幅用遮罩漸層淡入頁面背景（上方 40% 不變，往下漸淡），藝人名稱疊在淡出的部分，和 YTM 的藝人頁相同 | 藝人「en」：橫幅往下淡入黑色背景，名稱在淡出處 ✅ |
| 載入歌曲時沒有提示，停在 0:00 直到載入完成 | 播放器在真的開始前就回報「播放中」，所以播放鍵已是暫停圖示、時間停在 0:00 | 引擎的 `loading`：從點播（查 `/next`）開始，到新歌（或它的廣告）的時間真的開始走為止；中途緩衝（狀態 3）也算。使用者暫停、歌曲結束、停止時結束。播放頁與播放列的播放鍵在載入時換成轉圈 | 點「播放」後轉圈約 1 秒（播放器已啟動過），時間開始走就消失 ✅；跳到還沒緩衝的位置：轉圈 0.3 秒（狀態 3 → 1）✅ |

## 第十輪：使用者回報（播放頁收不起來、首頁主題按鈕、切換動畫、關閉動畫）

| 回報 | 原因 | 修正 | 驗證（平板） |
|---|---|---|---|
| 從首頁點歌開啟播放頁後無法收起 | 第八輪起點歌會先開播放頁，這時佇列還沒回來（`nowPlaying` 是空的）。播放頁「沒有歌就關閉」的檢查立刻執行，而 `openPlayer` 是先設成開啟、後推入歷史項目，所以 `closePlayer` 找不到播放頁那一層，在首頁（沒有任何層）時不斷呼叫自己而丟出錯誤，歷史項目也沒推入，之後按收起找不到可退的層 | `openPlayer` 先推入歷史項目再開啟；`closePlayer` 在歷史裡沒有播放頁時直接關閉；「沒有歌就關閉」只在沒有載入中時才算（第一首載入失敗仍會關閉）；載入第一首時播放頁顯示轉圈，不再顯示「目前沒有播放」 | 剛啟動、沒有播放時點首頁的歌 → 播放頁開啟並轉圈，歌曲出現後按收起 → 播放頁收起、播放列顯示該歌 ✅ |
| 首頁的主題按鈕在載入內容時消失 | 點主題按鈕是換掉整個頁面（`replacePage`），新頁面從「載入中」開始，按鈕要等內容回來才出現 | 分頁的主題按鈕改成在原頁面上篩選（`TabPage`）：頁面與按鈕列不重建（橫向捲動位置保留），點下的按鈕立即顯示為選取，舊內容變暗直到新內容回來；篩選仍記在路由上，重新載入頁面時保留 | 點「放鬆」：按鈕列是同一個元素、「放鬆」立即選取、舊內容變暗（opacity 0.4），約 0.8 秒後換成新內容 ✅；再點「放鬆」取消 → 回到原本內容 ✅ |
| 首頁切換內容需要轉換動畫 | — | 內容載入後以淡入加上移 12px 進場（220ms）；載入中的舊內容淡成 0.4。所有頁面第一次載入完成時也使用同一個進場動畫 | 取消篩選後新內容 opacity 0 → 0.39（60ms）→ 0.79（140ms）→ 1（260ms）✅ |
| 大部分介面關閉時直接消失，開啟時卻有動畫 | 各層用 `<Show>` 顯示，關閉時立即移除 | 新的 `Presence`（`ui/presence.tsx`）：關閉後保留最後的內容 180ms 並標上 `.leaving`，播放頁與選單往下滑出淡出、遮罩與提示訊息淡出，期間不接收點擊。用在播放頁、項目選單、「儲存至播放清單」、帳戶與設定選單、提示訊息 | 播放頁按收起、項目選單點遮罩：30ms 時 opacity 0.98、90ms 0.76、150ms 0.32、260ms 已移除 ✅；帳戶選單：60ms 時在離場中、300ms 已移除 ✅ |

頁面本身（推入新頁、返回）仍沒有進出場動畫，這輪沒有改。

測試時從首頁播放了「极乐之地」（會留在播放紀錄），測完已暫停。

## 第十一輪：使用者回報（廣告畫面消失、縮圖略過鈕、擴散效果頂端、帳戶選單）

| 回報 | 原因 | 修正 | 驗證（平板） |
|---|---|---|---|
| 有時廣告畫面消失，收起播放頁再打開才有畫面 | 播放器放在「最後註冊」的影片位置，而不是最上層的。播放頁開著時廣告開始，播放頁與播放列的位置會同時出現，順序不一定；播放列的位置後註冊時，影片就跑到播放頁底下的播放列 | 影片位置有層級（`AdVideoSlot` 的 `layer`：播放列 0、播放頁 1），取最高層，同層才看先後 | 播放頁開著時模擬廣告：影片在播放頁 ✅；收起 → 播放列 ✅；再打開 → 播放頁 ✅ |
| 廣告縮圖會顯示略過按鈕 | 播放器在播放列縮圖裡時，YouTube 自己的廣告介面（略過按鈕、廣告商連結）也一起顯示 | 縮圖裡的廣告介面設為透明且不接收點擊（`.video-ads` 等）。不用 `display: none`：播放列的「略過廣告」標籤要靠排版中的略過按鈕判斷能否略過、並把點擊轉給它 | 縮圖中的 `.video-ads`：opacity 0、pointer-events none ✅ |
| 播放頁最上緣畫不到擴散效果 | 寬版時 `.player-layout` 是 `overflow: hidden`，光暈在頂列下方被切成一條直線 | 寬版的 `.player-layout` 改成 `overflow: visible`（面板自己捲動，這裡沒有東西會溢出）；面板提高一層，光暈不會蓋到它 | 截圖：光暈一路延伸到螢幕頂端，頂列下沒有切線 ✅。窄版（直向）仍會在頂列下切掉：那裡版面要捲動，這輪沒改 |
| 帳戶選單大部分功能無法使用 | 新介面只處理會開成頁面的項目（你的頻道、觀看記錄）；其他是 YTM 自己的對話框或用戶端訊號，連結用 `window.open` 在 GeckoView 也打不開（沒有處理新視窗） | 依 YTM 的處理方式各自實作（`engine/account.ts`，對照 music_polymer 的處理程式），見下 | 見下 |

帳戶選單各項：

| 項目 | YTM 的端點 | 新介面 | 驗證 |
|---|---|---|---|
| 設定 | `applicationSettingsEndpoint` → `/account/get_setting` | 新的「設定」頁（`ui/settings.tsx`）：依分類列出開關、單選、連結、動作。伺服器設定用 `/account/set_setting`；觀看／搜尋記錄用 feedback token；語言、地區、嚴格篩選模式寫入 PREF cookie 後重新載入（和 YTM 相同）；YTM 需要先確認的（清除、暫停記錄）先跳出確認框 | 7 個分類都顯示，地區「台灣」、語言「中文（繁體）」由 cookie 讀出 ✅；「記住最近期使用的播放清單」關 → 伺服器回報 false，再開 → true ✅；「清除觀看記錄」跳出 YTM 的確認框，按取消關閉、沒有清除 ✅ |
| 切換帳戶 | `getAccountSwitcherEndpoint`（GET `/getAccountSwitcherEndpoint`） | 帳戶表單內換成帳戶清單：頭像、名稱、帳號代碼與訂閱數，目前的帳戶打勾；點其他帳戶走 YouTube 的登入處理（`signinUrl`）；「新增帳戶」到 Google 的登入頁（在 app 內） | 清單顯示唯一的頻道並標為目前帳戶、有「新增帳戶」✅。沒有其他帳戶，切換本身沒有測 |
| 登出 | `signOutEndpoint` → `LOGOUT_URL` | 先問「要登出 … 嗎？」再前往 `/logout` | 確認框出現，按取消仍是登入狀態 ✅（沒有真的登出） |
| 上傳歌曲 | 用戶端訊號 `OPEN_FILE_PICKER` | 從點擊本身開啟檔案選擇器（多選，flac/m4a/mp3/ogg/wma），上傳到 Google 的可續傳上傳服務（`upload.youtube.com/upload/usermusic/http`：先送檔名與大小取得上傳網址，再送檔案），提示「正在上傳／已上傳」。Android 端：GeckoPage 實作 `onFilePrompt`，用系統的文件選擇器，選到的檔案複製到快取再交給 Gecko | 真實點擊開啟系統選擇器，選了 6 秒的測試音檔 → 「已上傳」，媒體庫的上傳內容出現「Pear upload test」✅；測完已刪除（`/music/delete_privately_owned_entity`），原本的上傳內容不受影響 |
| 說明、條款和隱私權政策 | 用戶端訊號 `HELP`、`urlEndpoint` | 用系統瀏覽器開啟（說明中心）。Android 端：GeckoPage 把新視窗（`window.open`、`target=_blank`）交給系統瀏覽器 | 真實點擊「說明」→ 系統瀏覽器開啟 support.google.com ✅ |
| 提供意見 | `sendFeedbackAction` | YTM 用的是 Google 的頁內意見回饋元件，沒有它的介面就無法使用；改為開啟 YouTube Music 說明社群 | 和「說明」相同的開啟方式，沒有個別測 |

腳本發出的點擊沒有使用者手勢，Gecko 的彈出視窗封鎖會擋下 `window.open`；真實點擊才會開啟（已用真實點擊驗證）。

測試時真的遇到一則前置廣告，模擬廣告時和它重疊；廣告結束後歌曲照常載入播放。測完已暫停；系統瀏覽器留下一個說明中心的分頁。
