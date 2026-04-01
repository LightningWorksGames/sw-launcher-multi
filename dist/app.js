// ─── Tauri API (loaded after Tauri injects __TAURI__) ───────────────────────

let invoke, listen;

function tauriReady() {
  return !!(window.__TAURI__ && window.__TAURI__.core);
}

function ensureTauri() {
  if (!tauriReady()) {
    log('Tauri API not available yet.');
    return false;
  }
  if (!invoke) {
    invoke = window.__TAURI__.core.invoke;
    listen = window.__TAURI__.event.listen;
    setupTauriListeners();
  }
  return true;
}

// ─── DOM Elements ───────────────────────────────────────────────────────────

const installPathInput = document.getElementById('install-path');
const btnBrowse = document.getElementById('btn-browse');
const btnCheck = document.getElementById('btn-check');
const btnPlay = document.getElementById('btn-play');
const btnSignin = document.getElementById('btn-signin');
const btnSignout = document.getElementById('btn-signout');
const btnSettings = document.getElementById('btn-settings');
const btnSettingsSave = document.getElementById('btn-settings-save');
const btnSettingsCancel = document.getElementById('btn-settings-cancel');
const signedOutArea = document.getElementById('signed-out');
const signedInArea = document.getElementById('signed-in');
const userCic = document.getElementById('user-cic');
const userName = document.getElementById('user-name');
const progressContainer = document.getElementById('progress-container');
const progressFill = document.getElementById('progress-fill');
const progressText = document.getElementById('progress-text');
const consoleLog = document.getElementById('console-log');
const settingsPanel = document.getElementById('settings-panel');
const tabSlidesBtn = document.getElementById('tab-slides-btn');

// ─── Current user state ─────────────────────────────────────────────────────

let currentUser = null;

function isAdmin() {
  return currentUser && (currentUser.role === 'admin' || currentUser.role === 'superadmin');
}

// ─── Slideshow ──────────────────────────────────────────────────────────────

let currentSlide = 0;
let slideElements = document.querySelectorAll('.slide');

function nextSlide() {
  if (slideElements.length === 0) return;
  slideElements[currentSlide].classList.remove('active');
  currentSlide = (currentSlide + 1) % slideElements.length;
  slideElements[currentSlide].classList.add('active');
}

setInterval(nextSlide, 5000);

async function loadDynamicSlides() {
  if (!tauriReady() || !invoke) return;
  try {
    const urls = await invoke('fetch_slides');
    if (urls && urls.length > 0) {
      const slideshow = document.getElementById('slideshow');
      slideshow.innerHTML = '';
      urls.forEach((url, i) => {
        const img = document.createElement('img');
        img.className = 'slide' + (i === 0 ? ' active' : '');
        img.src = url;
        img.alt = '';
        slideshow.appendChild(img);
      });
      slideElements = document.querySelectorAll('.slide');
      currentSlide = 0;
    }
  } catch (e) {
    // Bundled slides remain as fallback
  }
}

// ─── Console Logging ────────────────────────────────────────────────────────

const MAX_LOG_LINES = 200;

function log(message) {
  const line = document.createElement('div');
  line.className = 'log-line';
  line.textContent = message;
  consoleLog.appendChild(line);
  while (consoleLog.children.length > MAX_LOG_LINES) {
    consoleLog.removeChild(consoleLog.firstChild);
  }
  consoleLog.parentElement.scrollTop = consoleLog.parentElement.scrollHeight;
}

// ─── Init ───────────────────────────────────────────────────────────────────

async function init() {
  if (!ensureTauri()) return;

  try {
    const settings = await invoke('get_settings');
    installPathInput.value = settings.install_path;
  } catch (e) {
    log('Failed to load settings: ' + e);
  }

  // Fetch shared greeting from Supabase
  let greeting = 'Launcher ready.';
  try {
    const config = await invoke('fetch_launcher_config');
    if (config.greeting) greeting = config.greeting;
  } catch (e) {}

  try {
    const auth = await invoke('get_stored_auth');
    if (auth.logged_in && auth.user) {
      showUser(auth.user);
      log('Signed in as ' + auth.user.display_name);
    }
  } catch (e) {}

  log(greeting);
  loadDynamicSlides();
}

function waitForTauri() {
  if (tauriReady()) {
    init();
  } else {
    log('Waiting for Tauri...');
    let attempts = 0;
    const interval = setInterval(() => {
      attempts++;
      if (tauriReady()) {
        clearInterval(interval);
        init();
      } else if (attempts > 50) {
        clearInterval(interval);
        log('Tauri API not found. Running in standalone mode.');
      }
    }, 100);
  }
}

// ─── User Display ───────────────────────────────────────────────────────────

function showUser(user) {
  currentUser = user;
  signedOutArea.style.display = 'none';
  signedInArea.style.display = 'flex';
  userName.textContent = user.display_name;

  if (isAdmin()) {
    tabSlidesBtn.style.display = '';
    btnSettings.style.display = '';
  } else {
    tabSlidesBtn.style.display = 'none';
    btnSettings.style.display = 'none';
  }

  const size = 40;
  const ringWidth = Math.max(2, Math.round(size * 0.03));
  const gapWidth = Math.max(1, Math.round(size * 0.01));

  userCic.innerHTML = '';
  userCic.style.width = size + 'px';
  userCic.style.height = size + 'px';
  userCic.style.borderRadius = '50%';
  userCic.style.border = ringWidth + 'px solid ' + user.avatar_outer_color;
  userCic.style.padding = gapWidth + 'px';
  userCic.style.background = '#000';
  userCic.style.overflow = 'hidden';

  const inner = document.createElement('div');
  inner.style.width = '100%';
  inner.style.height = '100%';
  inner.style.borderRadius = '50%';
  inner.style.border = ringWidth + 'px solid ' + user.avatar_inner_color;
  inner.style.overflow = 'hidden';

  if (user.avatar_url) {
    const img = document.createElement('img');
    img.src = user.avatar_url;
    img.style.width = '100%';
    img.style.height = '100%';
    img.style.objectFit = 'cover';
    const panX = ((user.avatar_pan_x || 0.5) - 0.5) * -100;
    const panY = ((user.avatar_pan_y || 0.5) - 0.5) * -100;
    const zoom = user.avatar_zoom || 1.0;
    img.style.transform = `scale(${zoom}) translate(${panX}%, ${panY}%)`;
    inner.appendChild(img);
  }

  userCic.appendChild(inner);
}

function showSignedOut() {
  currentUser = null;
  signedOutArea.style.display = 'flex';
  signedInArea.style.display = 'none';
  userName.textContent = '';
  userCic.innerHTML = '';
  tabSlidesBtn.style.display = 'none';
  btnSettings.style.display = 'none';
  switchTab('tab-settings');
}

// ─── Event Listeners ────────────────────────────────────────────────────────

btnBrowse.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  try {
    const path = await invoke('select_install_path');
    installPathInput.value = path;
    log('Install path set to: ' + path);
  } catch (e) {}
});

btnCheck.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  btnCheck.disabled = true;
  log('Checking for updates...');
  try {
    const result = await invoke('check_updates');
    log(result);
    if (result.includes('need updating')) {
      log('Click PLAY to download updates and launch.');
    }
  } catch (e) {
    log('Error: ' + e);
  }
  btnCheck.disabled = false;
});

btnPlay.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  btnPlay.disabled = true;
  btnPlay.textContent = 'UPDATING...';
  progressContainer.style.display = 'flex';
  try {
    await invoke('download_game');
    log('Launching game...');
    await invoke('launch_game');
  } catch (e) {
    log('Error: ' + e);
  }
  progressContainer.style.display = 'none';
  btnPlay.disabled = false;
  btnPlay.textContent = 'PLAY';
});

btnSignin.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  btnSignin.disabled = true;
  btnSignin.textContent = 'Waiting...';
  log('Opening browser for sign in... (2 min timeout)');
  invoke('start_sso_login').then(auth => {
    if (auth.logged_in && auth.user) {
      showUser(auth.user);
    }
    btnSignin.disabled = false;
    btnSignin.textContent = 'Sign In';
  }).catch(e => {
    log('Sign in failed: ' + e);
    btnSignin.disabled = false;
    btnSignin.textContent = 'Sign In';
  });
});

btnSignout.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  try {
    await invoke('logout');
    showSignedOut();
    log('Signed out.');
  } catch (e) {
    log('Error signing out: ' + e);
  }
});

// ─── Settings / Admin Panel ─────────────────────────────────────────────────

btnSettings.addEventListener('click', () => {
  settingsPanel.style.display = 'flex';
  switchTab('tab-settings');
  if (tauriReady() && invoke) {
    invoke('get_settings').then(settings => {
      document.getElementById('set-build-url').value = settings.build_server_url;
      document.getElementById('set-sso-url').value = settings.sso_url;
      document.getElementById('set-signing-identity').value = settings.signing_identity;
      document.getElementById('set-apple-team').value = settings.apple_team_id;
      document.getElementById('set-win-cert').value = settings.windows_cert_path;
    }).catch(() => {});
    // Load shared greeting from Supabase
    invoke('fetch_launcher_config').then(config => {
      document.getElementById('set-greeting').value = config.greeting || '';
    }).catch(() => {});
  }
});

btnSettingsSave.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  const settings = {
    install_path: installPathInput.value,
    build_server_url: document.getElementById('set-build-url').value,
    sso_url: document.getElementById('set-sso-url').value,
    signing_identity: document.getElementById('set-signing-identity').value,
    apple_team_id: document.getElementById('set-apple-team').value,
    windows_cert_path: document.getElementById('set-win-cert').value,
  };
  try {
    await invoke('save_settings', { settings });
    log('Settings saved.');
  } catch (e) {
    log('Failed to save settings: ' + e);
  }
  // Save greeting to Supabase separately (admin-only, may fail independently)
  if (isAdmin()) {
    try {
      const greeting = document.getElementById('set-greeting').value;
      await invoke('save_launcher_config', { config: { greeting } });
      log('Greeting updated for all users.');
    } catch (e) {
      log('Failed to save greeting: ' + e);
    }
  }
  settingsPanel.style.display = 'none';
});

btnSettingsCancel.addEventListener('click', () => {
  settingsPanel.style.display = 'none';
});

document.getElementById('btn-panel-close').addEventListener('click', () => {
  settingsPanel.style.display = 'none';
});

// ─── Tabs ───────────────────────────────────────────────────────────────────

function switchTab(tabId) {
  document.querySelectorAll('.panel-tab').forEach(t => t.classList.remove('active'));
  document.querySelectorAll('.tab-content').forEach(t => t.classList.remove('active'));
  const btn = document.querySelector(`.panel-tab[data-tab="${tabId}"]`);
  const content = document.getElementById(tabId);
  if (btn) btn.classList.add('active');
  if (content) content.classList.add('active');
  if (tabId === 'tab-slides') loadSlidesManager();
}

document.querySelectorAll('.panel-tab').forEach(tab => {
  tab.addEventListener('click', () => {
    const tabId = tab.getAttribute('data-tab');
    if (tabId === 'tab-slides' && !isAdmin()) return;
    switchTab(tabId);
  });
});

// ─── Slideshow Manager (Admin) ──────────────────────────────────────────────

const MAX_SLIDES = 6;
let currentSlideUrls = [];
let openDropdownCard = null;

async function loadSlidesManager() {
  const grid = document.getElementById('slides-grid');
  const status = document.getElementById('slides-status');
  grid.innerHTML = '<div class="slide-empty" style="grid-column:1/-1">Loading...</div>';
  status.textContent = '';

  try {
    const urls = await invoke('fetch_slides');
    currentSlideUrls = urls || [];
    renderSlidesGrid();
  } catch (e) {
    currentSlideUrls = [];
    renderSlidesGrid();
  }
}

function renderSlidesGrid() {
  const grid = document.getElementById('slides-grid');
  const status = document.getElementById('slides-status');
  grid.innerHTML = '';
  closeAllDropdowns();

  currentSlideUrls.forEach((url, i) => {
    const card = document.createElement('div');
    card.className = 'slide-card';
    card.draggable = true;
    card.dataset.index = i;

    const badge = document.createElement('div');
    badge.className = 'slide-order-badge';
    badge.textContent = i + 1;
    card.appendChild(badge);

    const img = document.createElement('img');
    img.src = url + '?t=' + Date.now();
    img.alt = '';
    card.appendChild(img);

    // Hamburger menu button
    const menuBtn = document.createElement('button');
    menuBtn.className = 'slide-menu-btn';
    menuBtn.innerHTML = '&#8942;'; // vertical ellipsis
    menuBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      toggleDropdown(card);
    });
    card.appendChild(menuBtn);

    // Dropdown menu
    const dropdown = document.createElement('div');
    dropdown.className = 'slide-dropdown';

    const viewBtn = document.createElement('button');
    viewBtn.textContent = 'View';
    viewBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      closeAllDropdowns();
      showFullscreen(url);
    });
    dropdown.appendChild(viewBtn);

    const replBtn = document.createElement('button');
    replBtn.textContent = 'Replace';
    replBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      closeAllDropdowns();
      openReplaceModal(url);
    });
    dropdown.appendChild(replBtn);

    const delBtn = document.createElement('button');
    delBtn.className = 'danger';
    delBtn.textContent = 'Delete';
    delBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      closeAllDropdowns();
      deleteSlide(url);
    });
    dropdown.appendChild(delBtn);

    card.appendChild(dropdown);

    // Drag events
    card.addEventListener('dragstart', onDragStart);
    card.addEventListener('dragend', onDragEnd);
    card.addEventListener('dragover', onDragOver);
    card.addEventListener('dragenter', onDragEnter);
    card.addEventListener('drop', onDrop);
    card.addEventListener('dragleave', onDragLeave);

    grid.appendChild(card);
  });

  for (let i = currentSlideUrls.length; i < MAX_SLIDES; i++) {
    const empty = document.createElement('div');
    empty.className = 'slide-card slide-empty';
    empty.textContent = '+ Add';
    empty.style.cursor = 'pointer';
    empty.addEventListener('click', () => {
      document.getElementById('slide-file-input').click();
    });
    grid.appendChild(empty);
  }

  status.textContent = currentSlideUrls.length + '/' + MAX_SLIDES + ' slides';

  const uploadBtn = document.getElementById('btn-upload-slide');
  if (currentSlideUrls.length >= MAX_SLIDES) {
    uploadBtn.style.opacity = '0.5';
    uploadBtn.style.pointerEvents = 'none';
  } else {
    uploadBtn.style.opacity = '1';
    uploadBtn.style.pointerEvents = 'auto';
  }
}

// ─── Dropdown Menus ─────────────────────────────────────────────────────────

function toggleDropdown(card) {
  const dd = card.querySelector('.slide-dropdown');
  const wasOpen = dd.classList.contains('open');
  closeAllDropdowns();
  if (!wasOpen) dd.classList.add('open');
}

function closeAllDropdowns() {
  document.querySelectorAll('.slide-dropdown.open').forEach(d => d.classList.remove('open'));
}

// Close dropdowns when clicking anywhere
document.addEventListener('click', closeAllDropdowns);

// ─── Fullscreen Preview ─────────────────────────────────────────────────────

function showFullscreen(url) {
  // Remove existing if any
  let existing = document.getElementById('slide-fullscreen');
  if (existing) existing.remove();

  const overlay = document.createElement('div');
  overlay.id = 'slide-fullscreen';
  const img = document.createElement('img');
  img.src = url + '?t=' + Date.now();
  img.alt = '';
  overlay.appendChild(img);
  overlay.addEventListener('click', () => overlay.remove());
  document.body.appendChild(overlay);
}

// ─── Drag and Drop Reordering ───────────────────────────────────────────────

let dragIndex = null;

function onDragStart(e) {
  dragIndex = parseInt(e.currentTarget.dataset.index);
  e.currentTarget.classList.add('dragging');
  e.dataTransfer.effectAllowed = 'move';
  // Required for Firefox
  e.dataTransfer.setData('text/plain', dragIndex.toString());
}

function onDragEnd(e) {
  e.currentTarget.classList.remove('dragging');
  document.querySelectorAll('.slide-card').forEach(c => c.classList.remove('drag-over'));
  dragIndex = null;
}

function onDragEnter(e) {
  e.preventDefault();
}

function onDragOver(e) {
  e.preventDefault();
  e.dataTransfer.dropEffect = 'move';
  const card = e.currentTarget;
  if (card.dataset.index !== undefined && parseInt(card.dataset.index) !== dragIndex) {
    card.classList.add('drag-over');
  }
}

function onDragLeave(e) {
  e.currentTarget.classList.remove('drag-over');
}

function onDrop(e) {
  e.preventDefault();
  e.currentTarget.classList.remove('drag-over');
  const dropIndex = parseInt(e.currentTarget.dataset.index);

  if (dragIndex === null || isNaN(dropIndex) || dragIndex === dropIndex) return;

  const item = currentSlideUrls.splice(dragIndex, 1)[0];
  currentSlideUrls.splice(dropIndex, 0, item);
  renderSlidesGrid();
  saveSlideOrder();
}

async function saveSlideOrder() {
  if (!ensureTauri()) return;
  try {
    const filenames = currentSlideUrls.map(url => url.split('/').pop().split('?')[0]);
    await invoke('save_slide_order', { order: filenames });
  } catch (e) {
    log('Failed to save slide order: ' + e);
  }
}

// ─── Slide Upload / Delete / Replace ────────────────────────────────────────

async function deleteSlide(url) {
  if (!ensureTauri()) return;
  const filename = url.split('/').pop().split('?')[0];
  if (!confirm('Delete ' + filename + '?')) return;

  const status = document.getElementById('slides-status');
  status.textContent = 'Deleting...';

  try {
    await invoke('delete_slide', { filename });
    loadSlidesManager();
    loadDynamicSlides();
  } catch (e) {
    status.textContent = 'Delete failed: ' + e;
  }
}

document.getElementById('slide-file-input').addEventListener('change', async (e) => {
  const file = e.target.files[0];
  if (!file) return;
  if (!ensureTauri()) return;

  const status = document.getElementById('slides-status');
  status.textContent = 'Uploading ' + file.name + '...';

  try {
    const arrayBuffer = await file.arrayBuffer();
    const data = Array.from(new Uint8Array(arrayBuffer));

    const ext = file.name.split('.').pop().toLowerCase();
    const baseName = file.name.replace(/\.[^.]+$/, '')
      .replace(/[^a-zA-Z0-9_-]/g, '_')
      .toLowerCase();
    const filename = baseName + '.' + ext;

    await invoke('upload_slide', { filename, data });
    status.textContent = 'Uploaded!';
    loadSlidesManager();
    loadDynamicSlides();
  } catch (e) {
    status.textContent = 'Upload failed: ' + e;
  }

  e.target.value = '';
});

// ─── Replace Slide ──────────────────────────────────────────────────────────

let replaceUrl = null;

function openReplaceModal(url) {
  replaceUrl = url;
  const modal = document.getElementById('replace-modal');
  const preview = document.getElementById('replace-preview');
  const status = document.getElementById('replace-status');
  preview.innerHTML = '<img src="' + url + '?t=' + Date.now() + '" alt="" />';
  status.textContent = 'Choose a new image to replace this slide.';
  modal.style.display = 'flex';
}

document.getElementById('btn-replace-cancel').addEventListener('click', () => {
  document.getElementById('replace-modal').style.display = 'none';
  replaceUrl = null;
});

document.getElementById('replace-file-input').addEventListener('change', async (e) => {
  const file = e.target.files[0];
  if (!file || replaceUrl === null) return;
  if (!ensureTauri()) return;

  const status = document.getElementById('replace-status');
  status.textContent = 'Replacing...';

  try {
    const oldFilename = replaceUrl.split('/').pop().split('?')[0];
    const arrayBuffer = await file.arrayBuffer();
    const data = Array.from(new Uint8Array(arrayBuffer));

    await invoke('upload_slide', { filename: oldFilename, data });

    status.textContent = 'Replaced!';
    document.getElementById('replace-modal').style.display = 'none';
    replaceUrl = null;
    loadSlidesManager();
    loadDynamicSlides();
  } catch (e) {
    status.textContent = 'Replace failed: ' + e;
  }

  e.target.value = '';
});

// ─── Tauri Event Listeners ──────────────────────────────────────────────────

function setupTauriListeners() {
  listen('log', (event) => {
    log(event.payload);
  });

  listen('download-progress', (event) => {
    const { current, total, file } = event.payload;
    const pct = Math.round((current / total) * 100);
    progressFill.style.width = pct + '%';
    progressText.textContent = pct + '%';
    progressContainer.style.display = 'flex';

    if (current >= total) {
      setTimeout(() => {
        progressContainer.style.display = 'none';
      }, 2000);
    }
  });
}

// ─── Start ──────────────────────────────────────────────────────────────────

waitForTauri();
