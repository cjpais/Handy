// HandySTT Sidebar - Main Logic
// Manages native messaging, recording, transcription, and actions

// ============================================================================
// State Management
// ============================================================================

let port = null;
let isRecording = false;
let isConnected = false;
let reconnectAttempts = 0;
const MAX_RECONNECT_ATTEMPTS = 5;

// ============================================================================
// Initialization
// ============================================================================

document.addEventListener('DOMContentLoaded', () => {
  initializeUI();
  loadSettings();
  loadHistory();
  connectToNativeApp();
});

function initializeUI() {
  // Recording control
  document.getElementById('record-btn').addEventListener('click', toggleRecording);

  // Actions
  document.getElementById('paste-btn').addEventListener('click', handlePaste);
  document.getElementById('send-btn').addEventListener('click', handleSendToAI);

  // Settings
  document.getElementById('settings-btn').addEventListener('click', openSettings);
  document.getElementById('close-settings').addEventListener('click', closeSettings);

  // History
  document.getElementById('clear-history').addEventListener('click', clearHistory);

  // Error toast
  document.getElementById('close-error').addEventListener('click', hideError);

  // Settings changes
  document.getElementById('auto-paste').addEventListener('change', saveSettings);
  document.getElementById('auto-send-ai').addEventListener('change', saveSettings);
  document.getElementById('model-select').addEventListener('change', saveSettings);
  document.getElementById('custom-ai-url').addEventListener('change', saveSettings);
  document.getElementById('ai-target').addEventListener('change', saveSettings);
}

// ============================================================================
// Native Messaging
// ============================================================================

function connectToNativeApp() {
  try {
    updateConnectionStatus('connecting');

    port = chrome.runtime.connectNative('com.pais.handy.host');

    port.onMessage.addListener(handleNativeMessage);

    port.onDisconnect.addListener(() => {
      const error = chrome.runtime.lastError;
      console.error('Native host disconnected:', error);

      isConnected = false;
      updateConnectionStatus('disconnected');
      disableRecordingUI();

      // Attempt reconnection
      if (reconnectAttempts < MAX_RECONNECT_ATTEMPTS) {
        reconnectAttempts++;
        setTimeout(() => {
          console.log(`Reconnection attempt ${reconnectAttempts}/${MAX_RECONNECT_ATTEMPTS}`);
          connectToNativeApp();
        }, 2000 * reconnectAttempts);
      } else {
        showError('Cannot connect to Handy desktop app. Please ensure it is installed and running.');
      }
    });

    // Send initial handshake
    sendToNativeApp({ command: 'handshake' });

  } catch (error) {
    console.error('Failed to connect to native host:', error);
    isConnected = false;
    updateConnectionStatus('error');
    showError('Failed to connect to Handy. Is it installed?');
  }
}

function handleNativeMessage(message) {
  console.log('Received from native:', message);

  switch (message.type) {
    case 'handshake_ack':
      isConnected = true;
      reconnectAttempts = 0;
      updateConnectionStatus('connected');
      enableRecordingUI();
      break;

    case 'transcription':
      handleTranscription(message.text);
      break;

    case 'recording_status':
      updateRecordingStatus(message.isRecording);
      break;

    case 'error':
      handleNativeError(message.error);
      break;

    case 'partial_transcription':
      updatePartialTranscription(message.text);
      break;

    default:
      console.warn('Unknown message type:', message.type);
  }
}

function sendToNativeApp(command) {
  if (port) {
    try {
      port.postMessage(command);
      console.log('Sent to native:', command);
    } catch (error) {
      console.error('Failed to send message to native host:', error);
      showError('Communication error with Handy app');
    }
  } else {
    console.error('No connection to native host');
    showError('Not connected to Handy app');
  }
}

// ============================================================================
// Recording Control
// ============================================================================

function toggleRecording() {
  if (!isConnected) {
    showError('Not connected to Handy app');
    return;
  }

  isRecording = !isRecording;

  if (isRecording) {
    startRecording();
  } else {
    stopRecording();
  }
}

function startRecording() {
  sendToNativeApp({ command: 'start_recording' });
  updateRecordingUI('recording');
}

function stopRecording() {
  sendToNativeApp({ command: 'stop_recording' });
  updateRecordingUI('processing');
}

function updateRecordingStatus(recording) {
  isRecording = recording;
  if (recording) {
    updateRecordingUI('recording');
  } else {
    updateRecordingUI('ready');
  }
}

function updateRecordingUI(state) {
  const indicator = document.getElementById('recording-indicator');
  const statusText = document.getElementById('status-text');
  const recordBtn = document.getElementById('record-btn');

  switch (state) {
    case 'recording':
      indicator.className = 'recording';
      statusText.textContent = 'Recording...';
      recordBtn.textContent = 'Stop Recording';
      recordBtn.disabled = false;
      break;

    case 'processing':
      indicator.className = 'processing';
      statusText.textContent = 'Processing...';
      recordBtn.disabled = true;
      break;

    case 'ready':
    default:
      indicator.className = 'inactive';
      statusText.textContent = 'Ready';
      recordBtn.textContent = 'Start Recording';
      recordBtn.disabled = false;
      isRecording = false;
      break;
  }
}

function updateConnectionStatus(status) {
  const statusEl = document.getElementById('connection-status');
  const textEl = document.getElementById('connection-text');

  switch (status) {
    case 'connected':
      statusEl.className = 'connection-status connected';
      textEl.textContent = 'Connected to Handy';
      break;

    case 'connecting':
      statusEl.className = 'connection-status connecting';
      textEl.textContent = 'Connecting...';
      break;

    case 'disconnected':
      statusEl.className = 'connection-status disconnected';
      textEl.textContent = 'Disconnected';
      break;

    case 'error':
      statusEl.className = 'connection-status error';
      textEl.textContent = 'Connection Error';
      break;
  }
}

function enableRecordingUI() {
  document.getElementById('record-btn').disabled = false;
}

function disableRecordingUI() {
  document.getElementById('record-btn').disabled = true;
  updateRecordingUI('ready');
}

// ============================================================================
// Transcription Handling
// ============================================================================

function handleTranscription(text) {
  if (!text || text.trim() === '') {
    console.warn('Empty transcription received');
    updateRecordingUI('ready');
    return;
  }

  displayTranscription(text);
  addToHistory(text);

  // Auto-actions
  chrome.storage.local.get(['autoPaste', 'autoSendAI'], (result) => {
    if (result.autoPaste) {
      pasteToActiveField(text);
    }

    if (result.autoSendAI) {
      sendToAI(text);
    }
  });

  updateRecordingUI('ready');
}

function updatePartialTranscription(text) {
  const textarea = document.getElementById('transcription');
  textarea.value = text;
}

function displayTranscription(text) {
  const textarea = document.getElementById('transcription');
  textarea.value = text;
  enableActionButtons();
}

function enableActionButtons() {
  document.getElementById('paste-btn').disabled = false;
  document.getElementById('send-btn').disabled = false;
}

// ============================================================================
// History Management
// ============================================================================

async function addToHistory(text) {
  const historyList = document.getElementById('history-list');

  // Create history item
  const item = document.createElement('div');
  item.className = 'history-item';
  item.textContent = text.substring(0, 60) + (text.length > 60 ? '...' : '');
  item.title = text; // Full text on hover

  item.addEventListener('click', () => {
    document.getElementById('transcription').value = text;
    enableActionButtons();
  });

  historyList.insertBefore(item, historyList.firstChild);

  // Save to storage
  await saveToStorage(text);
}

async function saveToStorage(text) {
  const result = await chrome.storage.local.get('history');
  const history = result.history || [];

  history.unshift({
    text: text,
    timestamp: Date.now()
  });

  // Keep last 50 items
  if (history.length > 50) {
    history.pop();
  }

  await chrome.storage.local.set({ history });
}

async function loadHistory() {
  const result = await chrome.storage.local.get('history');
  const history = result.history || [];

  const historyList = document.getElementById('history-list');
  historyList.innerHTML = '';

  history.forEach(entry => {
    const item = document.createElement('div');
    item.className = 'history-item';
    item.textContent = entry.text.substring(0, 60) + (entry.text.length > 60 ? '...' : '');
    item.title = entry.text;

    item.addEventListener('click', () => {
      document.getElementById('transcription').value = entry.text;
      enableActionButtons();
    });

    historyList.appendChild(item);
  });
}

async function clearHistory() {
  if (!confirm('Clear all transcription history?')) {
    return;
  }

  await chrome.storage.local.set({ history: [] });
  document.getElementById('history-list').innerHTML = '';
}

// ============================================================================
// Paste to Active Field
// ============================================================================

async function handlePaste() {
  const text = document.getElementById('transcription').value;
  if (!text || text.trim() === '') {
    showError('No text to paste');
    return;
  }

  await pasteToActiveField(text);
}

async function pasteToActiveField(text) {
  try {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });

    if (!tab) {
      throw new Error('No active tab found');
    }

    await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: pasteTextInPage,
      args: [text]
    });

  } catch (error) {
    console.error('Paste error:', error);
    // Fallback to clipboard
    await navigator.clipboard.writeText(text);
    showError('Copied to clipboard (no active field detected)');
  }
}

// This function runs in the page context
function pasteTextInPage(text) {
  const activeElement = document.activeElement;

  if (activeElement && (
    activeElement.tagName === 'TEXTAREA' ||
    activeElement.tagName === 'INPUT' ||
    activeElement.contentEditable === 'true'
  )) {
    if (activeElement.contentEditable === 'true') {
      // For contentEditable elements
      document.execCommand('insertText', false, text);
    } else {
      // For input/textarea
      const start = activeElement.selectionStart || 0;
      const end = activeElement.selectionEnd || 0;
      const value = activeElement.value || '';

      activeElement.value = value.substring(0, start) + text + value.substring(end);
      activeElement.selectionStart = activeElement.selectionEnd = start + text.length;

      // Trigger input event
      activeElement.dispatchEvent(new Event('input', { bubbles: true }));
    }
  } else {
    // Copy to clipboard as fallback
    navigator.clipboard.writeText(text);
    alert('Text copied to clipboard (no active input field detected)');
  }
}

// ============================================================================
// Send to AI Integration
// ============================================================================

async function handleSendToAI() {
  const text = document.getElementById('transcription').value;
  if (!text || text.trim() === '') {
    showError('No text to send');
    return;
  }

  await sendToAI(text);
}

async function sendToAI(text) {
  const target = document.getElementById('ai-target').value;

  // AI URLs
  const aiUrls = {
    'claude': 'https://claude.ai',
    'chatgpt': 'https://chatgpt.com',
    'gemini': 'https://gemini.google.com'
  };

  let url;

  if (target === 'custom') {
    const customUrl = await getCustomAIUrl();
    if (!customUrl) {
      showError('Please set a custom AI URL in settings');
      return;
    }
    url = customUrl;
  } else {
    url = aiUrls[target];
  }

  if (!url) {
    showError('Invalid AI target');
    return;
  }

  try {
    // Find existing tab with the AI URL
    const tabs = await chrome.tabs.query({ url: url + '/*' });

    if (tabs.length > 0) {
      // Focus existing tab
      await chrome.tabs.update(tabs[0].id, { active: true });
      await chrome.windows.update(tabs[0].windowId, { focused: true });

      // Wait a bit for tab to be ready
      setTimeout(() => injectTextToAI(tabs[0].id, text), 500);
    } else {
      // Open new tab
      const newTab = await chrome.tabs.create({ url: url });

      // Wait for page to load
      chrome.tabs.onUpdated.addListener(function listener(tabId, info) {
        if (tabId === newTab.id && info.status === 'complete') {
          chrome.tabs.onUpdated.removeListener(listener);
          setTimeout(() => injectTextToAI(newTab.id, text), 1000);
        }
      });
    }
  } catch (error) {
    console.error('Send to AI error:', error);
    showError('Failed to send to AI: ' + error.message);
  }
}

async function injectTextToAI(tabId, text) {
  try {
    await chrome.scripting.executeScript({
      target: { tabId: tabId },
      func: insertIntoAIPrompt,
      args: [text]
    });
  } catch (error) {
    console.error('Inject error:', error);
    // Copy to clipboard as fallback
    await navigator.clipboard.writeText(text);
    showError('Text copied to clipboard (could not inject into AI page)');
  }
}

// This function runs in the AI page context
function insertIntoAIPrompt(text) {
  // Try multiple selectors for different AI interfaces
  const selectors = [
    // Claude
    'div[contenteditable="true"][role="textbox"]',
    'textarea[placeholder*="reply"]',
    'textarea[placeholder*="Reply"]',
    // ChatGPT
    'textarea[placeholder*="Message"]',
    'textarea[placeholder*="message"]',
    '#prompt-textarea',
    // Gemini
    'div[contenteditable="true"]',
    'textarea',
    // Generic
    'input[type="text"]'
  ];

  for (const selector of selectors) {
    const elements = document.querySelectorAll(selector);

    for (const element of elements) {
      // Check if element is visible
      if (element.offsetParent === null) continue;

      if (element.tagName === 'TEXTAREA' || element.tagName === 'INPUT') {
        element.value = text;
        element.focus();
      } else if (element.contentEditable === 'true') {
        element.textContent = text;
        element.focus();
      }

      // Trigger input event
      element.dispatchEvent(new Event('input', { bubbles: true }));
      element.dispatchEvent(new Event('change', { bubbles: true }));

      return; // Exit after first successful injection
    }
  }

  // If no element found, copy to clipboard
  navigator.clipboard.writeText(text);
  alert('Text copied to clipboard (could not find AI input field)');
}

async function getCustomAIUrl() {
  const result = await chrome.storage.local.get('customAIUrl');
  return result.customAIUrl || '';
}

// ============================================================================
// Settings Management
// ============================================================================

function openSettings() {
  document.getElementById('settings-panel').classList.remove('hidden');
}

function closeSettings() {
  document.getElementById('settings-panel').classList.add('hidden');
}

async function saveSettings() {
  const settings = {
    autoPaste: document.getElementById('auto-paste').checked,
    autoSendAI: document.getElementById('auto-send-ai').checked,
    model: document.getElementById('model-select').value,
    customAIUrl: document.getElementById('custom-ai-url').value,
    aiTarget: document.getElementById('ai-target').value
  };

  await chrome.storage.local.set(settings);

  // Send model selection to native app
  if (isConnected) {
    sendToNativeApp({
      command: 'set_model',
      model: settings.model
    });
  }
}

async function loadSettings() {
  const result = await chrome.storage.local.get([
    'autoPaste',
    'autoSendAI',
    'model',
    'customAIUrl',
    'aiTarget'
  ]);

  document.getElementById('auto-paste').checked = result.autoPaste || false;
  document.getElementById('auto-send-ai').checked = result.autoSendAI || false;
  document.getElementById('model-select').value = result.model || 'parakeet-v3';
  document.getElementById('custom-ai-url').value = result.customAIUrl || '';
  document.getElementById('ai-target').value = result.aiTarget || 'claude';
}

// ============================================================================
// Error Handling
// ============================================================================

function handleNativeError(error) {
  console.error('Native error:', error);

  const errorMessages = {
    'no_microphone': 'No microphone detected. Please check your audio input.',
    'model_not_found': 'STT model not installed. Please install it in Handy.',
    'transcription_failed': 'Transcription failed. Please try again.',
    'permission_denied': 'Microphone permission denied.'
  };

  const message = errorMessages[error] || `Error: ${error}`;
  showError(message);

  updateRecordingUI('ready');
}

function showError(message) {
  const toast = document.getElementById('error-toast');
  const messageEl = document.getElementById('error-message');

  messageEl.textContent = message;
  toast.classList.remove('hidden');

  // Auto-hide after 5 seconds
  setTimeout(hideError, 5000);
}

function hideError() {
  document.getElementById('error-toast').classList.add('hidden');
}
