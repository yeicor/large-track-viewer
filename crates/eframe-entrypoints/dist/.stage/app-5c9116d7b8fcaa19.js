// Set footer year dynamically for polish
(() => {
  const year = new Date().getFullYear();
  const footerYear = document.getElementById("footer_year");
  if (footerYear) footerYear.textContent = year;
})();

// Helper to log and show errors to the user
function showError(message, callstack = null) {
  const centerText = document.getElementById("center_text");
  if (centerText) {
    const appName =
      centerText.querySelector("h1")?.textContent || "Application";
    centerText.style.display = "flex";
    centerText.setAttribute("aria-busy", "false");
    centerText.innerHTML = `
            <div class="lds-dual-ring" style="display:none"></div>
            <h1 style="margin-bottom: 0.5em;">${appName}</h1>
            <div class="error-message">Error: ${message}</div>
            ${callstack ? `<pre class="error-stack">${callstack}</pre>` : ""}
            <button class="reload-btn" id="reload_btn" tabindex="0" autofocus>Reload</button>
        `;
    const reloadBtn = document.getElementById("reload_btn");
    if (reloadBtn) {
      reloadBtn.onclick = () => location.reload();
      reloadBtn.onkeydown = (e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          location.reload();
        }
      };
    }
  }
  console.error("Application error:", message, callstack || "");
}

// Show a smooth fade-out for the loading overlay
function hideLoadingOverlay() {
  const centerText = document.getElementById("center_text");
  if (centerText) {
    centerText.setAttribute("aria-busy", "false");
    centerText.style.transition = "opacity 0.5s";
    centerText.style.opacity = "0";
    setTimeout(() => {
      centerText.style.display = "none";
    }, 500);
  }
}

// ============================================
// Native App Download Banner
// ============================================

function detectPlatform() {
  const userAgent = navigator.userAgent.toLowerCase();
  const platform = navigator.platform?.toLowerCase() || "";

  if (/android/.test(userAgent)) {
    return { os: "android", arch: null, ext: ".apk" };
  }

  if (/iphone|ipad|ipod/.test(userAgent)) {
    return { os: "ios", arch: null, ext: null }; // iOS not supported via direct download
  }

  if (/win/.test(platform) || /win/.test(userAgent)) {
    return { os: "windows", arch: "x86_64", ext: ".exe" };
  }

  if (/mac/.test(platform) || /mac/.test(userAgent)) {
    // Check for Apple Silicon vs Intel
    // Note: This is a best guess, as JS can't reliably detect CPU architecture
    return { os: "macos", arch: "x86_64", ext: "" };
  }

  if (/linux/.test(platform) || /linux/.test(userAgent)) {
    return { os: "linux", arch: "x86_64", ext: "" };
  }

  return { os: "unknown", arch: null, ext: null };
}

function getAssetPattern(platform) {
  switch (platform.os) {
    case "windows":
      return /x86_64-pc-windows.*\.exe$/i;
    case "macos":
      return /x86_64-apple-darwin[^.]*$/i;
    case "linux":
      return /x86_64-unknown-linux-gnu[^.]*$/i;
    case "android":
      return /\.apk$/i;
    default:
      return null;
  }
}

function getPlatformDisplayName(platform) {
  switch (platform.os) {
    case "windows":
      return "Windows";
    case "macos":
      return "macOS";
    case "linux":
      return "Linux";
    case "android":
      return "Android";
    default:
      return null;
  }
}

async function fetchLatestRelease(githubRepo) {
  try {
    const response = await fetch(
      `https://api.github.com/repos/${githubRepo}/releases/latest`,
      {
        headers: {
          Accept: "application/vnd.github.v3+json",
        },
      },
    );

    if (!response.ok) {
      console.warn("Failed to fetch latest release:", response.status);
      return null;
    }

    return await response.json();
  } catch (error) {
    console.warn("Error fetching release:", error);
    return null;
  }
}

function findMatchingAsset(release, platform) {
  const pattern = getAssetPattern(platform);
  if (!pattern || !release?.assets) return null;

  for (const asset of release.assets) {
    if (pattern.test(asset.name)) {
      return asset;
    }
  }
  return null;
}

async function initNativeAppBanner() {
  const banner = document.getElementById("native-app-banner");
  const downloadLink = document.getElementById("native-app-download-link");
  const dismissBtn = document.getElementById("native-app-banner-dismiss");
  const bannerText = banner?.querySelector(".native-app-banner-text");

  if (!banner || !downloadLink || !dismissBtn) return;

  // Get config from body data attributes
  const githubRepo = document.body.dataset.githubRepo;
  const appName = document.body.dataset.appName || "the native app";

  if (!githubRepo) {
    console.warn("No github-repo data attribute found on body");
    return;
  }

  // Check if user has dismissed the banner
  const dismissKey = `native-app-banner-dismissed-${githubRepo}`;
  if (localStorage.getItem(dismissKey)) {
    return;
  }

  // Detect platform
  const platform = detectPlatform();
  const platformName = getPlatformDisplayName(platform);

  // Don't show banner for unsupported platforms
  if (!platformName) {
    return;
  }

  // Fetch latest release from GitHub
  const release = await fetchLatestRelease(githubRepo);

  if (!release) {
    // Fallback to releases page
    downloadLink.href = `https://github.com/${githubRepo}/releases/latest`;
    if (bannerText) {
      bannerText.textContent = `Get better performance with ${appName} for ${platformName}!`;
    }
    banner.style.display = "block";
  } else {
    // Try to find a matching asset for the platform
    const asset = findMatchingAsset(release, platform);

    if (asset) {
      downloadLink.href = asset.browser_download_url;
      if (bannerText) {
        bannerText.textContent = `Get better performance with ${appName} for ${platformName}!`;
      }
    } else {
      // Fallback to releases page if no matching asset
      downloadLink.href = `https://github.com/${githubRepo}/releases/latest`;
      if (bannerText) {
        bannerText.textContent = `Get better performance with ${appName}!`;
      }
    }

    banner.style.display = "block";
  }

  // Handle dismiss
  dismissBtn.onclick = () => {
    banner.style.transition = "opacity 0.3s, transform 0.3s";
    banner.style.opacity = "0";
    banner.style.transform = "translateX(-50%) translateY(20px)";
    setTimeout(() => {
      banner.style.display = "none";
    }, 300);
    localStorage.setItem(dismissKey, "true");
  };
}

// ============================================
// Main App Initialization
// ============================================

// Wait for TrunkApplicationStarted before doing anything else
function onTrunkStarted(event) {
  // Import the WASM JS glue code (Trunk will generate this)
  const get_web_handle = window.wasmBindings.get_web_handle;

  async function main() {
    try {
      // Set up the Rust WebHandle
      const handle = get_web_handle();

      // Get the canvas element
      const canvas = document.getElementById("the_canvas_id");
      if (!canvas) {
        showError("Canvas element not found.");
        return;
      }

      // Monitor for panics and show details if they occur
      function checkPanic() {
        if (handle.has_panicked()) {
          const msg = handle.panic_message() || "Unknown panic";
          const stack = handle.panic_callstack() || "";
          showError(msg, stack);
        } else {
          setTimeout(checkPanic, 1000);
        }
      }
      setTimeout(checkPanic, 1000);

      // Actually start the Rust app
      await handle.start(canvas);

      hideLoadingOverlay();

      // Show native app banner after app has loaded (non-blocking)
      setTimeout(() => {
        initNativeAppBanner();
      }, 2000); // Wait 2 seconds after load to not be intrusive
    } catch (e) {
      let msg = e && e.message ? e.message : String(e);
      let stack = e && e.stack ? e.stack : "";
      showError(msg, stack);
    }
  }

  if (document.readyState === "loading") {
    window.addEventListener("DOMContentLoaded", main);
  } else {
    main();
  }
}

// Listen for TrunkApplicationStarted before accessing window or doing anything else
window.addEventListener("TrunkApplicationStarted", onTrunkStarted, {
  once: true,
});
