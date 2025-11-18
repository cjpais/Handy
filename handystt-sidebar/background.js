// HandySTT Sidebar - Background Service Worker
// Handles extension installation, updates, and side panel management

// ============================================================================
// Installation & Updates
// ============================================================================

chrome.runtime.onInstalled.addListener((details) => {
  if (details.reason === 'install') {
    console.log('HandySTT Sidebar installed');
    onFirstInstall();
  } else if (details.reason === 'update') {
    console.log('HandySTT Sidebar updated to version', chrome.runtime.getManifest().version);
    onUpdate();
  }
});

async function onFirstInstall() {
  // Set default settings
  await chrome.storage.local.set({
    autoPaste: false,
    autoSendAI: false,
    model: 'parakeet-v3',
    aiTarget: 'claude',
    customAIUrl: '',
    history: []
  });

  // Open welcome/setup page
  chrome.tabs.create({
    url: chrome.runtime.getURL('welcome.html')
  });
}

async function onUpdate() {
  // Handle any migration needed for settings
  const result = await chrome.storage.local.get();

  // Ensure new settings have defaults
  const updates = {};
  if (!result.hasOwnProperty('model')) {
    updates.model = 'parakeet-v3';
  }
  if (!result.hasOwnProperty('aiTarget')) {
    updates.aiTarget = 'claude';
  }

  if (Object.keys(updates).length > 0) {
    await chrome.storage.local.set(updates);
  }
}

// ============================================================================
// Side Panel Management
// ============================================================================

// Open side panel when extension icon is clicked
chrome.action.onClicked.addListener(async (tab) => {
  try {
    // Open the side panel on the current window
    await chrome.sidePanel.open({ windowId: tab.windowId });
  } catch (error) {
    console.error('Failed to open side panel:', error);
  }
});

// Enable side panel for all tabs
chrome.runtime.onStartup.addListener(() => {
  console.log('HandySTT Sidebar started');
});

// ============================================================================
// Message Handling
// ============================================================================

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  console.log('Background received message:', message);

  switch (message.type) {
    case 'get_active_tab':
      getActiveTab().then(sendResponse);
      return true; // Will respond asynchronously

    case 'open_settings':
      openSettings();
      break;

    case 'check_native_host':
      checkNativeHost().then(sendResponse);
      return true;

    default:
      console.warn('Unknown message type:', message.type);
  }
});

async function getActiveTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab;
}

function openSettings() {
  chrome.runtime.openOptionsPage();
}

async function checkNativeHost() {
  try {
    const port = chrome.runtime.connectNative('com.pais.handy.host');

    return new Promise((resolve) => {
      const timeout = setTimeout(() => {
        port.disconnect();
        resolve({ connected: false, error: 'Connection timeout' });
      }, 5000);

      port.onMessage.addListener((message) => {
        clearTimeout(timeout);
        port.disconnect();
        resolve({ connected: true });
      });

      port.onDisconnect.addListener(() => {
        clearTimeout(timeout);
        resolve({
          connected: false,
          error: chrome.runtime.lastError?.message || 'Disconnected'
        });
      });

      // Send test message
      port.postMessage({ command: 'ping' });
    });
  } catch (error) {
    return { connected: false, error: error.message };
  }
}

// ============================================================================
// Context Menu (Optional)
// ============================================================================

chrome.runtime.onInstalled.addListener(() => {
  // Create context menu item to open sidebar
  chrome.contextMenus.create({
    id: 'open-handystt',
    title: 'Open HandySTT Sidebar',
    contexts: ['all']
  });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === 'open-handystt') {
    chrome.sidePanel.open({ windowId: tab.windowId });
  }
});

// ============================================================================
// Keyboard Shortcut (Optional)
// ============================================================================

chrome.commands.onCommand.addListener((command) => {
  if (command === 'toggle-sidebar') {
    chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
      if (tabs[0]) {
        chrome.sidePanel.open({ windowId: tabs[0].windowId });
      }
    });
  }
});

// ============================================================================
// Keep Service Worker Alive (if needed)
// ============================================================================

// Periodic ping to keep service worker alive
// Note: This is generally not needed for side panels, but can help with native messaging
let keepAliveInterval;

function startKeepAlive() {
  keepAliveInterval = setInterval(() => {
    console.log('Keep alive ping');
  }, 20000); // Every 20 seconds
}

function stopKeepAlive() {
  if (keepAliveInterval) {
    clearInterval(keepAliveInterval);
  }
}

// Start keep-alive when native connection is established
chrome.runtime.onConnect.addListener((port) => {
  if (port.name === 'keepAlive') {
    startKeepAlive();
    port.onDisconnect.addListener(stopKeepAlive);
  }
});

console.log('HandySTT Sidebar background service worker loaded');
