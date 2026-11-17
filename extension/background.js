importScripts("token.js");

const HOST_NAME = "com.adm.downloader";
const CONTEXT_MENU_ID = "adm-download";
const BROWSER_CONTEXT_MENU_ID = "browser-download";
const handled = new Set();
// 右键“使用浏览器下载”是扩展主动发起的下载，本次必须完全绕过接管。
// byExtensionId 是主判断，URL 短时白名单是为了兼容极少数 byExtensionId
// 还没填上的早期事件；命中后立即删除，不影响后续正常点击同一链接。
const browserBypassUrls = new Map();
const BROWSER_BYPASS_TTL_MS = 5000;

let supportedFormats = null;
let admEnabled = false;

function updateActionState() {
  chrome.action.setBadgeText({ text: admEnabled ? "" : "OFF" });
  chrome.action.setBadgeBackgroundColor({ color: "#777777" });
  chrome.action.setTitle({
    title: admEnabled
      ? "ADM Downloader - 正在接管浏览器下载"
      : "ADM Downloader - 已暂停接管",
  });
}

const settingsReady = chrome.storage.local
  .get({ admEnabled: true })
  .then((value) => {
    admEnabled = value.admEnabled !== false;
    updateActionState();
  })
  .catch((error) => {
    console.warn("[ADM] read settings failed, disable takeover:", error);
    admEnabled = false;
    updateActionState();
  });

chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local" || !changes.admEnabled) {
    return;
  }
  admEnabled = changes.admEnabled.newValue !== false;
  updateActionState();
});

chrome.action.onClicked.addListener(() => {
  void settingsReady.then(() => {
    admEnabled = !admEnabled;
    updateActionState();
    chrome.storage.local.set({ admEnabled });
  });
});

function normalizeFormat(value) {
  return String(value).trim().replace(/^\./, "").toLowerCase();
}

function loadFormats() {
  chrome.runtime.sendNativeMessage(
    HOST_NAME,
    { action: "formats" },
    (response) => {
      if (chrome.runtime.lastError) {
        console.warn("[ADM] formats unavailable:", chrome.runtime.lastError.message);
        return;
      }
      if (response && response.ok && Array.isArray(response.formats)) {
        supportedFormats = response.formats.map(normalizeFormat).filter(Boolean);
        console.log("[ADM] supported formats:", supportedFormats);
      }
    },
  );
}

loadFormats();

function setupContextMenu() {
  chrome.contextMenus.removeAll(() => {
    void chrome.runtime.lastError;
    chrome.contextMenus.create(
      {
        id: CONTEXT_MENU_ID,
        title: "使用 ADM 下载",
        contexts: ["link", "image", "video", "audio"],
      },
      () => {
        if (chrome.runtime.lastError) {
          console.warn(
            "[ADM] ADM context menu create failed:",
            chrome.runtime.lastError.message,
          );
        }
      },
    );
    chrome.contextMenus.create(
      {
        id: BROWSER_CONTEXT_MENU_ID,
        title: "使用浏览器下载",
        contexts: ["link", "image", "video", "audio"],
      },
      () => {
        if (chrome.runtime.lastError) {
          console.warn(
            "[ADM] browser context menu create failed:",
            chrome.runtime.lastError.message,
          );
        }
      },
    );
  });
}

function markBrowserBypass(url) {
  const now = Date.now();
  for (const [key, expiresAt] of browserBypassUrls) {
    if (expiresAt <= now) {
      browserBypassUrls.delete(key);
    }
  }
  browserBypassUrls.set(url, now + BROWSER_BYPASS_TTL_MS);
}

function consumeBrowserBypass(item) {
  // byExtensionId 是权威判断。命中时顺便清掉 URL 白名单，避免同一链接
  // 短时间内被用户正常点击时也误绕过。
  if (item.byExtensionId === chrome.runtime.id) {
    browserBypassUrls.clear();
    return true;
  }
  const now = Date.now();
  for (const [url, expiresAt] of browserBypassUrls) {
    if (expiresAt <= now) {
      browserBypassUrls.delete(url);
      continue;
    }
    if (item.url === url || item.finalUrl === url) {
      browserBypassUrls.delete(url);
      return true;
    }
  }
  return false;
}

chrome.runtime.onInstalled.addListener(setupContextMenu);
chrome.runtime.onInstalled.addListener(updateActionState);
chrome.runtime.onStartup.addListener(setupContextMenu);
chrome.runtime.onStartup.addListener(updateActionState);
setupContextMenu();

function fileNameFromUrl(url) {
  try {
    const parsed = new URL(url);
    const raw = parsed.pathname.split("/").pop() || "";
    return decodeURIComponent(raw);
  } catch {
    return "";
  }
}

function readCookies(url, callback) {
  chrome.cookies.getAll({ url }, (cookies) => {
    if (chrome.runtime.lastError) {
      console.warn("[ADM] cookie read failed:", chrome.runtime.lastError.message);
      callback("");
      return;
    }
    const header = (cookies || [])
      .filter((cookie) => cookie && cookie.name)
      .map((cookie) => `${cookie.name}=${cookie.value || ""}`)
      .join("; ")
      .replace(/[\r\n]/g, "");
    callback(header);
  });
}

function handoffUrl(url, referrer) {
  if (!/^https?:\/\//i.test(url)) {
    console.log("[ADM] context menu ignored unsupported URL:", url);
    return;
  }

  readCookies(url, (cookies) => {
    console.log("[ADM] context menu cookies:", cookies ? "present" : "empty");
    const payload = {
      action: "download",
      token: IPC_TOKEN,
      url,
      filename: fileNameFromUrl(url),
      referrer: referrer || "",
      cookies,
      userAgent: navigator.userAgent,
      fileSize: 0,
    };

    chrome.runtime.sendNativeMessage(HOST_NAME, payload, (response) => {
      if (chrome.runtime.lastError) {
        console.warn("[ADM] context menu native host error:", chrome.runtime.lastError.message);
        return;
      }
      console.log("[ADM] context menu handed off:", response);
    });
  });
}

// 在页面上下文中执行，不能用 downloads.download 代替。
// 优先点击原来的 <a>，保留页面自己的下载处理（包括 blob 下载）；
// 找不到对应元素时创建一个带 download 属性的临时链接触发浏览器原生下载。
function triggerPageBrowserDownload(downloadUrl) {
  try {
    const absolute = new URL(downloadUrl, location.href).href;
    const protocol = new URL(absolute).protocol.toLowerCase();
    if (!["http:", "https:", "blob:", "data:", "file:"].includes(protocol)) {
      return "";
    }

    const anchor = Array.from(document.querySelectorAll("a[href]")).find(
      (item) => item.href === absolute,
    );
    if (anchor) {
      anchor.click();
      return "anchor";
    }

    const fallback = document.createElement("a");
    fallback.href = absolute;
    fallback.download = "";
    fallback.style.display = "none";
    (document.body || document.documentElement).appendChild(fallback);
    fallback.click();
    fallback.remove();
    return "created";
  } catch (error) {
    return "";
  }
}

chrome.contextMenus.onClicked.addListener((info, tab) => {
  const url = info.linkUrl || info.srcUrl || "";
  if (!url) {
    return;
  }

  if (info.menuItemId === BROWSER_CONTEXT_MENU_ID) {
    markBrowserBypass(url);

    const openInTab = () => {
      chrome.tabs.create({ url, active: false }, () => {
        if (chrome.runtime.lastError) {
          browserBypassUrls.delete(url);
          console.warn(
            "[ADM] browser tab download fallback failed:",
            chrome.runtime.lastError.message,
          );
        }
      });
    };

    if (!tab || typeof tab.id !== "number") {
      openInTab();
      return;
    }

    const target = { tabId: tab.id };
    if (typeof info.frameId === "number") {
      target.frameIds = [info.frameId];
    }

    // 不要用 downloads.download 下载页面生成的 blob/data 链接：扩展没有
    // 页面上下文的权限，会出现“无法下载 - 没有权限”。在页面里触发原生
    // 链接点击，让浏览器按自己的下载流程处理，才能带上页面上下文。
    chrome.scripting.executeScript(
      {
        target,
        func: triggerPageBrowserDownload,
        args: [url],
      },
      (results) => {
        if (chrome.runtime.lastError) {
          console.warn(
            "[ADM] browser menu script failed:",
            chrome.runtime.lastError.message,
          );
          openInTab();
          return;
        }
        const result = results && results[0] ? results[0].result : "";
        console.log("[ADM] browser menu native trigger:", result);
        if (!result) {
          openInTab();
        }
      },
    );
    return;
  }

  if (info.menuItemId !== CONTEXT_MENU_ID) {
    return;
  }
  handoffUrl(url, info.pageUrl || (tab && tab.url) || "");
});

function itemExtension(item) {
  const name = item.filename || "";
  let path = name;
  if (!path && item.url) {
    try {
      path = new URL(item.url).pathname;
    } catch {
      path = item.url;
    }
  }
  const clean = path.split("?")[0].split("#")[0];
  const dot = clean.lastIndexOf(".");
  if (dot < 0 || dot === clean.length - 1) {
    return "";
  }
  return clean.slice(dot + 1).toLowerCase();
}

function shouldHandle(item) {
  // 留空表示接管所有下载。
  if (!supportedFormats || supportedFormats.length === 0) {
    return true;
  }
  const ext = itemExtension(item);
  return ext !== "" && supportedFormats.includes(ext);
}

function eraseBrowserDownload(id) {
  chrome.downloads.erase({ id }, () => {
    void chrome.runtime.lastError;
  });
}

function cancelBrowserDownload(id) {
  chrome.downloads.search({ id }, (items) => {
    if (chrome.runtime.lastError) {
      void chrome.runtime.lastError;
      eraseBrowserDownload(id);
      return;
    }
    const state = items && items[0] ? items[0].state : "";
    if (state !== "in_progress") {
      eraseBrowserDownload(id);
      return;
    }
    chrome.downloads.cancel(id, () => {
      // 下载可能刚好完成，cancel 会报 "Download must be in progress"。
      void chrome.runtime.lastError;
      eraseBrowserDownload(id);
    });
  });
}

async function handoff(item) {
  if (!item || !item.url || handled.has(item.id)) {
    return;
  }
  // Mark every event we see immediately. This also covers events that arrive
  // while takeover is disabled, so replaying the same id after re-enabling
  // cannot turn it into a new takeover.
  handled.add(item.id);

  // 右键“使用浏览器下载”只放行本次下载；不读 Cookie、不取消、不发给 ADM。
  // URL 白名单命中后立即删除，后续正常点击同一链接仍会走接管逻辑。
  if (consumeBrowserBypass(item)) {
    console.log("[ADM] browser-menu download bypassed:", item.id);
    return;
  }

  // Never intercept downloads issued by this extension itself.
  if (item.byExtensionId === chrome.runtime.id) {
    return;
  }
  // Chrome/Edge can restore old download items and replay download events
  // after a browser restart. Only a live, unpaused download is eligible.
  if (item.state && item.state !== "in_progress") {
    console.log("[ADM] ignore non-running download event:", item.id, item.state);
    return;
  }
  if (item.paused) {
    console.log("[ADM] ignore paused download event:", item.id);
    return;
  }
  const startedAt = Date.parse(item.startTime || "");
  if (Number.isFinite(startedAt) && Date.now() - startedAt > 60_000) {
    console.log("[ADM] ignore stale download event:", item.id);
    return;
  }

  await settingsReady;
  if (!admEnabled) {
    console.log("[ADM] takeover disabled, browser download continues:", item.url);
    return;
  }
  // ADM 的 HTTP 引擎只能处理 http/https；blob/data/file 交给浏览器自己下载。
  let protocol = "";
  try {
    protocol = new URL(item.url).protocol.toLowerCase();
  } catch {
    return;
  }
  if (protocol !== "http:" && protocol !== "https:") {
    console.log("[ADM] browser handles unsupported protocol:", item.url);
    return;
  }
  if (!shouldHandle(item)) {
    console.log("[ADM] ignored unsupported download:", item.filename || item.url);
    return;
  }

  readCookies(item.url, (cookies) => {
    console.log("[ADM] download cookies:", cookies ? "present" : "empty");
    const payload = {
      action: "download",
      token: IPC_TOKEN,
      url: item.url,
      filename: item.filename || "",
      referrer: item.referrer || "",
      cookies,
      userAgent: navigator.userAgent,
      fileSize: item.fileSize || 0,
    };

    // 先取消浏览器自己的下载，再把链接交给 ADM。
    cancelBrowserDownload(item.id);

    chrome.runtime.sendNativeMessage(HOST_NAME, payload, (response) => {
      if (chrome.runtime.lastError) {
        console.warn("[ADM] native host error:", chrome.runtime.lastError.message);
        return;
      }
      console.log("[ADM] download handed off:", response);
    });
  });
}

// Chromium/Edge 只使用 onDeterminingFilename。
//
// 不能在这里同时监听 downloads.onCreated：Chrome/Edge 在浏览器启动、
// 恢复下载或关闭浏览器时会补发历史/已完成下载的 onCreated 事件。
// 如果补发时接管开关已经被重新打开，就会把上一次浏览器自己完成的下载
// 再次交给 ADM。参考扩展在 Chromium 上也只注册这个事件。
const determiningEvent = chrome.downloads?.onDeterminingFilename;
if (determiningEvent) {
  determiningEvent.addListener((item, suggest) => {
    void handoff(item);
    suggest();
  });
} else if (chrome.downloads?.onCreated) {
  // 只有不支持 onDeterminingFilename 的浏览器才用 onCreated 兜底。
  // handoff() 里会过滤已完成、暂停、过期和自身扩展的下载。
  chrome.downloads.onCreated.addListener((item) => {
    void handoff(item);
  });
}
