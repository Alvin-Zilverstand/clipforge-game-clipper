import { convertFileSrc } from "@tauri-apps/api/core";

type RecordingState =
  | "WaitingForGame"
  | "Buffering"
  | "RecordingSession"
  | "Clipping"
  | "Processing"
  | "StorageLow";

type ClipSource = "Auto Event" | "Manual Hotkey" | "Bookmark" | "Imported";
type UploadState = "Local only" | "Uploaded" | "Failed" | "Queued";

type Clip = {
  id: string;
  title: string;
  game: string;
  event: string;
  source: ClipSource;
  duration: string;
  createdAt: string;
  uploadState: UploadState;
  path: string;
  thumbnailPath: string | null;
  videoUrl: string | null;
  thumbnailUrl: string | null;
  tags: string[];
  colorClass: string;
};

type AutoEvent = {
  id: string;
  game: string;
  event: string;
  enabled: boolean;
};

type DesktopStatus = {
  recording_state: RecordingState;
  detected_game: string | null;
  replay_buffer_seconds: number;
  mic_enabled: boolean;
  mic_device: string | null;
  auto_record_enabled: boolean;
  upload_enabled: boolean;
  session_recording: boolean;
  capture_active: boolean;
  capture_path: string | null;
  clip_count: number;
  library_root: string;
  ffmpeg_available: boolean;
  ffmpeg_path: string | null;
  system_audio_available: boolean;
  desktop_duplication_available: boolean;
};

type ClipDto = {
  id: string;
  title: string;
  game: string;
  event: string;
  source: ClipSource;
  duration: string;
  created_at: string;
  upload_state: UploadState;
  path: string;
  thumbnail_path: string | null;
  tags: string[];
  color_class: string;
};

type GsiConfigDto = {
  game_id: string;
  path: string;
};

type AudioDeviceDto = {
  name: string;
  kind: "input" | "system_loopback";
};

type AppState = {
  activeView: "Library" | "Recording" | "Auto Clip" | "Uploads" | "Settings";
  recordingState: RecordingState;
  detectedGame: string;
  replayBufferSeconds: number;
  sessionRecording: boolean;
  captureActive: boolean;
  capturePath: string | null;
  ffmpegAvailable: boolean;
  ffmpegPath: string | null;
  systemAudioAvailable: boolean;
  desktopDuplicationAvailable: boolean;
  micEnabled: boolean;
  micDevice: string;
  audioDevices: AudioDeviceDto[];
  autoRecordEnabled: boolean;
  autoUpload: boolean;
  uploadProvider: "catbox" | "litterbox" | "custom_http" | "lustful";
  customUploadEndpoint: string;
  customUploadResponsePath: string;
  gsiConfigStatus: string;
  selectedClipId: string;
  clips: Clip[];
  autoEvents: AutoEvent[];
};

type TauriGlobal = {
  core?: {
    invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  };
};

declare global {
  interface Window {
    __TAURI__?: TauriGlobal;
    __TAURI_INTERNALS__?: unknown;
  }
}

let tauriInvoke: (<T>(command: string, args?: Record<string, unknown>) => Promise<T>) | null = null;

const state: AppState = {
  activeView: "Library",
  recordingState: "Buffering",
  detectedGame: "Counter-Strike 2",
  replayBufferSeconds: 60,
  sessionRecording: false,
  captureActive: false,
  capturePath: null,
  ffmpegAvailable: false,
  ffmpegPath: null,
  systemAudioAvailable: false,
  desktopDuplicationAvailable: false,
  micEnabled: false,
  micDevice: "",
  audioDevices: [],
  autoRecordEnabled: false,
  autoUpload: false,
  uploadProvider: "catbox",
  customUploadEndpoint: "",
  customUploadResponsePath: "url",
  gsiConfigStatus: "",
  selectedClipId: "clip-1",
  clips: [
    {
      id: "clip-1",
      title: "Dust II clutch",
      game: "Counter-Strike 2",
      event: "Kill",
      source: "Auto Event",
      duration: "0:18",
      createdAt: "Today 12:08",
      uploadState: "Local only",
      path: "",
      thumbnailPath: null,
      videoUrl: null,
      thumbnailUrl: null,
      tags: ["clutch", "kill"],
      colorClass: "kill",
    },
    {
      id: "clip-2",
      title: "Baron steal",
      game: "League of Legends",
      event: "Objective",
      source: "Auto Event",
      duration: "0:22",
      createdAt: "Today 11:44",
      uploadState: "Queued",
      path: "",
      thumbnailPath: null,
      videoUrl: null,
      thumbnailUrl: null,
      tags: ["objective"],
      colorClass: "objective",
    },
    {
      id: "clip-3",
      title: "Manual save",
      game: "Dota 2",
      event: "Manual",
      source: "Manual Hotkey",
      duration: "1:00",
      createdAt: "Yesterday 22:15",
      uploadState: "Uploaded",
      path: "",
      thumbnailPath: null,
      videoUrl: null,
      thumbnailUrl: null,
      tags: ["teamfight"],
      colorClass: "manual",
    },
  ],
  autoEvents: [
    { id: "cs2-kill", game: "Counter-Strike 2", event: "Kill", enabled: true },
    { id: "cs2-round", game: "Counter-Strike 2", event: "Round win", enabled: true },
    { id: "lol-kill", game: "League of Legends", event: "Champion kill", enabled: true },
    { id: "lol-objective", game: "League of Legends", event: "Objective", enabled: true },
    { id: "dota-objective", game: "Dota 2", event: "Roshan/objective", enabled: false },
  ],
};

const root = document.querySelector<HTMLDivElement>("#app");

if (!root) {
  throw new Error("Missing #app root");
}

const appRoot = root;

function render() {
  const selectedClip = state.clips.find((clip) => clip.id === state.selectedClipId) ?? state.clips[0];
  appRoot.innerHTML = `
    <main class="app-shell">
      <aside class="sidebar" aria-label="Primary">
        <div class="brand">
          <span class="brand-mark">CF</span>
          <span>ClipForge</span>
        </div>
        <nav>${["Library", "Recording", "Auto Clip", "Uploads", "Settings"]
          .map(
            (view) => `
              <button class="nav-item ${state.activeView === view ? "active" : ""}" data-view="${view}">
                ${view}
              </button>
            `,
          )
          .join("")}</nav>
      </aside>

      <section class="workspace">
        ${renderRecordingBar()}
        ${renderActiveView(selectedClip)}
      </section>
    </main>
  `;
  bindEvents();
}

function renderRecordingBar() {
  const statusText = state.captureActive
    ? "Recording to disk"
    : state.sessionRecording
      ? "Recording session"
      : `${state.recordingState} ${state.replayBufferSeconds}s`;
  return `
    <header class="recording-bar">
      <div>
        <p class="eyebrow">Detected game</p>
        <h1>${state.detectedGame}</h1>
      </div>
      <div class="meter-group" aria-label="Audio levels">
        <span class="meter"><i style="width: 72%"></i></span>
        <span class="meter"><i style="width: ${state.micEnabled ? 34 : 0}%"></i></span>
      </div>
      <div class="status-pill">${statusText}</div>
      <button class="icon-button" data-action="clip">Clip</button>
      <button class="record-button" data-action="toggle-record">${state.captureActive || state.sessionRecording ? "Stop" : "Record"}</button>
    </header>
  `;
}

function renderActiveView(selectedClip: Clip) {
  switch (state.activeView) {
    case "Recording":
      return renderRecordingView();
    case "Auto Clip":
      return renderAutoClipView();
    case "Uploads":
      return renderUploadsView();
    case "Settings":
      return renderSettingsView();
    case "Library":
    default:
      return renderLibraryView(selectedClip);
  }
}

function renderLibraryView(selectedClip: Clip | undefined) {
  return `
    <section class="content-grid">
      <section class="library-panel">
        <div class="section-heading">
          <div>
            <p class="eyebrow">Local library</p>
            <h2>Recent Clips</h2>
          </div>
          <div class="segmented">
            <button class="selected">Grid</button>
            <button>List</button>
          </div>
        </div>
        <div class="filters">
          <button class="selected">All</button>
          <button>Kills</button>
          <button>Wins</button>
          <button>Uploaded</button>
        </div>
        <div class="clip-grid">
          ${state.clips.length > 0 ? state.clips.map(renderClipCard).join("") : `<div class="empty-state">No clips saved yet</div>`}
        </div>
      </section>
      ${renderClipDetails(selectedClip)}
    </section>
  `;
}

function clipFromDto(dto: ClipDto): Clip {
  return {
    id: dto.id,
    title: dto.title,
    game: dto.game,
    event: dto.event,
    source: dto.source,
    duration: dto.duration,
    createdAt: dto.created_at,
    uploadState: dto.upload_state,
    path: dto.path,
    thumbnailPath: dto.thumbnail_path,
    videoUrl: dto.path ? convertFileSrc(dto.path) : null,
    thumbnailUrl: dto.thumbnail_path ? convertFileSrc(dto.thumbnail_path) : null,
    tags: dto.tags,
    colorClass: dto.color_class,
  };
}

function renderClipCard(clip: Clip) {
  return `
    <article class="clip-card ${state.selectedClipId === clip.id ? "selected" : ""}" data-clip-id="${clip.id}">
      <div class="thumb ${clip.colorClass}">
        ${clip.thumbnailUrl ? `<img src="${clip.thumbnailUrl}" alt="" />` : clip.event}
      </div>
      <h3>${clip.title}</h3>
      <p>${clip.game} · ${clip.source} · ${clip.duration}</p>
    </article>
  `;
}

function renderClipDetails(clip: Clip | undefined) {
  if (!clip) {
    return `
      <aside class="detail-panel">
        <div class="preview"><span>Preview</span></div>
        <h2>No clip selected</h2>
        <p class="muted">Start recording or press Clip to create the first local clip.</p>
      </aside>
    `;
  }

  return `
    <aside class="detail-panel">
      <div class="preview">
        ${
          clip.videoUrl
            ? `<video controls preload="metadata" src="${clip.videoUrl}" poster="${clip.thumbnailUrl ?? ""}"></video>`
            : `<span>Preview</span>`
        }
      </div>
      <h2>${clip.title}</h2>
      <p class="muted">${clip.game} · ${clip.event} · ${clip.createdAt} · ${clip.uploadState}</p>
      <div class="tag-row">${clip.tags.map((tag) => `<span>${tag}</span>`).join("")}</div>
      <p class="muted file-path">${clip.path || "Demo clip; no local file yet"}</p>
      <div class="trim-row">
        <label>Start <input value="00:00" data-action="trim-start" /></label>
        <label>End <input value="${clip.duration}" data-action="trim-end" /></label>
      </div>
      <div class="actions">
        <button data-action="process">Trim</button>
        <button data-action="reveal">Reveal</button>
        <button data-action="upload">Upload</button>
        <button data-action="delete">Delete</button>
      </div>
      <section class="auto-box">
        <h3>Auto clip policy</h3>
        <p>10s pre-roll · 8s post-roll · 12s merge window</p>
      </section>
    </aside>
  `;
}

function renderRecordingView() {
  return `
    <section class="settings-grid">
      <article class="settings-panel">
        <p class="eyebrow">Replay buffer</p>
        <h2>${state.replayBufferSeconds} seconds</h2>
        <input type="range" min="15" max="600" step="15" value="${state.replayBufferSeconds}" data-action="buffer-length" />
      </article>
      <article class="settings-panel">
        <p class="eyebrow">Capture</p>
        <h2>720p30 Performance</h2>
        <p class="muted">Lightweight desktop capture uses bundled FFmpeg when system FFmpeg is missing.</p>
        <p class="muted">${state.desktopDuplicationAvailable ? "Desktop Duplication capture available" : "Using GDI capture fallback"}</p>
        <p class="muted">${state.ffmpegAvailable ? "FFmpeg ready" : "FFmpeg missing"}${state.ffmpegPath ? ` · ${state.ffmpegPath}` : ""}</p>
        <p class="muted">${state.capturePath ? `Writing ${state.capturePath}` : "No active capture file"}</p>
      </article>
      <article class="settings-panel">
        <p class="eyebrow">Audio</p>
        <h2>${state.systemAudioAvailable ? "System audio ready" : "System audio unavailable"}</h2>
        <p class="muted">${state.systemAudioAvailable ? "FFmpeg reports WASAPI input support." : "This FFmpeg build does not expose WASAPI; video recording still works."}</p>
        <label class="toggle"><input type="checkbox" ${state.micEnabled ? "checked" : ""} data-action="toggle-mic" /> Mic capture</label>
        <label>Mic device
          <select data-action="mic-device">
            <option value="">Default microphone</option>
            ${state.audioDevices
              .filter((device) => device.kind === "input")
              .map(
                (device) =>
                  `<option value="${escapeHtml(device.name)}" ${state.micDevice === device.name ? "selected" : ""}>${escapeHtml(device.name)}</option>`,
              )
              .join("")}
          </select>
        </label>
      </article>
    </section>
  `;
}

function renderAutoClipView() {
  return `
    <section class="table-panel">
      <div class="section-heading">
        <div>
          <p class="eyebrow">Game events</p>
          <h2>Auto Clip Rules</h2>
        </div>
        <div class="status-pill">Local APIs only</div>
      </div>
      <div class="event-list">
        ${state.autoEvents
          .map(
            (event) => `
              <label class="event-row">
                <span><strong>${event.game}</strong><small>${event.event}</small></span>
                <input type="checkbox" ${event.enabled ? "checked" : ""} data-event-id="${event.id}" />
              </label>
            `,
          )
          .join("")}
      </div>
      <div class="actions">
        <button data-action="write-gsi-configs">Write GSI Configs</button>
      </div>
      ${state.gsiConfigStatus ? `<p class="muted file-path">${state.gsiConfigStatus}</p>` : ""}
    </section>
  `;
}

function renderUploadsView() {
  const providers: Array<{ id: AppState["uploadProvider"]; title: string; body: string }> = [
    { id: "catbox", title: "Catbox", body: "Anonymous or userhash-backed permanent uploads." },
    { id: "litterbox", title: "Litterbox", body: "Temporary uploads with a 24 hour default expiry." },
    { id: "custom_http", title: "Custom HTTP", body: "POST multipart clip files to your own endpoint." },
    { id: "lustful", title: "Lustful", body: "Blocked until API details are confirmed." },
  ];
  return `
    <section class="settings-grid">
      ${providers.map((provider) => `
        <article class="settings-panel">
          <p class="eyebrow">Provider</p>
          <h2>${provider.title}</h2>
          <p class="muted">${provider.body}</p>
          <button class="${state.uploadProvider === provider.id ? "selected" : ""}" data-upload-provider="${provider.id}">
            ${state.uploadProvider === provider.id ? "Selected" : "Select"}
          </button>
        </article>
      `).join("")}
      <article class="settings-panel">
        <p class="eyebrow">Custom HTTP</p>
        <h2>Endpoint</h2>
        <label>URL <input value="${state.customUploadEndpoint}" data-action="custom-upload-endpoint" /></label>
        <label>Response path <input value="${state.customUploadResponsePath}" data-action="custom-upload-response-path" /></label>
      </article>
    </section>
  `;
}

function renderSettingsView() {
  return `
    <section class="settings-grid">
      <article class="settings-panel">
        <p class="eyebrow">Privacy</p>
        <h2>Local-first</h2>
        <label class="toggle"><input type="checkbox" ${state.autoRecordEnabled ? "checked" : ""} data-action="toggle-auto-record" /> Start recording when a supported game is detected</label>
        <label class="toggle"><input type="checkbox" ${state.autoUpload ? "checked" : ""} data-action="toggle-auto-upload" /> Auto-upload after clipping</label>
      </article>
      <article class="settings-panel">
        <p class="eyebrow">Storage</p>
        <h2>50 GB cap</h2>
        <p class="muted">Old temporary buffer segments are removed automatically; saved clips are kept.</p>
      </article>
      <article class="settings-panel">
        <p class="eyebrow">Hotkeys</p>
        <h2>F8 saves last 60s</h2>
        <p class="muted">Shift+F8 saves 30s · Alt+F7 toggles session recording.</p>
      </article>
    </section>
  `;
}

function bindEvents() {
  appRoot.querySelectorAll<HTMLButtonElement>("[data-view]").forEach((button) => {
    button.addEventListener("click", () => {
      state.activeView = button.dataset.view as AppState["activeView"];
      render();
    });
  });

  appRoot.querySelectorAll<HTMLElement>("[data-clip-id]").forEach((card) => {
    card.addEventListener("click", () => {
      state.selectedClipId = card.dataset.clipId ?? state.selectedClipId;
      render();
    });
  });

  appRoot.querySelector<HTMLButtonElement>("[data-action='clip']")?.addEventListener("click", () => {
    void saveClip();
  });

  appRoot.querySelector<HTMLButtonElement>("[data-action='toggle-record']")?.addEventListener("click", () => {
    void toggleRecording();
  });

  appRoot.querySelector<HTMLButtonElement>("[data-action='process']")?.addEventListener("click", () => {
    void trimSelectedClip();
  });

  appRoot.querySelector<HTMLButtonElement>("[data-action='reveal']")?.addEventListener("click", () => {
    void revealSelectedClip();
  });

  appRoot.querySelector<HTMLButtonElement>("[data-action='upload']")?.addEventListener("click", () => {
    void uploadSelectedClip();
  });

  appRoot.querySelector<HTMLButtonElement>("[data-action='write-gsi-configs']")?.addEventListener("click", () => {
    void writeGsiConfigs();
  });

  appRoot.querySelector<HTMLButtonElement>("[data-action='delete']")?.addEventListener("click", () => {
    void deleteSelectedClip();
  });

  appRoot.querySelector<HTMLInputElement>("[data-action='buffer-length']")?.addEventListener("input", (event) => {
    state.replayBufferSeconds = Number((event.target as HTMLInputElement).value);
    render();
  });
  appRoot.querySelector<HTMLInputElement>("[data-action='buffer-length']")?.addEventListener("change", (event) => {
    void saveReplayBufferSetting(Number((event.target as HTMLInputElement).value));
  });

  appRoot.querySelector<HTMLInputElement>("[data-action='toggle-mic']")?.addEventListener("change", (event) => {
    state.micEnabled = (event.target as HTMLInputElement).checked;
    render();
    void saveMicSetting(state.micEnabled);
  });

  appRoot.querySelector<HTMLSelectElement>("[data-action='mic-device']")?.addEventListener("change", (event) => {
    state.micDevice = (event.target as HTMLSelectElement).value;
    void saveMicDevice(state.micDevice);
  });

  appRoot.querySelector<HTMLInputElement>("[data-action='toggle-auto-upload']")?.addEventListener("change", (event) => {
    state.autoUpload = (event.target as HTMLInputElement).checked;
    render();
  });

  appRoot.querySelector<HTMLInputElement>("[data-action='toggle-auto-record']")?.addEventListener("change", (event) => {
    state.autoRecordEnabled = (event.target as HTMLInputElement).checked;
    render();
    void saveAutoRecordSetting(state.autoRecordEnabled);
  });

  appRoot.querySelectorAll<HTMLButtonElement>("[data-upload-provider]").forEach((button) => {
    button.addEventListener("click", () => {
      state.uploadProvider = button.dataset.uploadProvider as AppState["uploadProvider"];
      render();
    });
  });

  appRoot.querySelector<HTMLInputElement>("[data-action='custom-upload-endpoint']")?.addEventListener("input", (event) => {
    state.customUploadEndpoint = (event.target as HTMLInputElement).value;
  });

  appRoot.querySelector<HTMLInputElement>("[data-action='custom-upload-response-path']")?.addEventListener("input", (event) => {
    state.customUploadResponsePath = (event.target as HTMLInputElement).value;
  });

  appRoot.querySelectorAll<HTMLInputElement>("[data-event-id]").forEach((input) => {
    input.addEventListener("change", () => {
      const eventRule = state.autoEvents.find((event) => event.id === input.dataset.eventId);
      if (eventRule) {
        eventRule.enabled = input.checked;
      }
    });
  });
}

async function saveClip() {
  state.recordingState = "Clipping";
  render();

  if (tauriInvoke) {
    try {
      const dto = await tauriInvoke<ClipDto>("save_manual_clip", { seconds: state.replayBufferSeconds });
      const clip = clipFromDto(dto);
      state.clips.unshift(clip);
      state.selectedClipId = clip.id;
      state.activeView = "Library";
      state.recordingState = "Processing";
      await refreshClips();
      render();
      return;
    } catch (error) {
      console.error("Could not save desktop clip", error);
    }
  }

  state.clips.unshift({
    id: `manual-${Date.now()}`,
    title: "Manual clip",
    game: state.detectedGame,
    event: "Manual",
    source: "Manual Hotkey",
    duration: "1:00",
    createdAt: "Just now",
    uploadState: "Local only",
    path: "",
    thumbnailPath: null,
    videoUrl: null,
    thumbnailUrl: null,
    tags: ["manual"],
    colorClass: "manual",
  });
  state.selectedClipId = state.clips[0].id;
  state.activeView = "Library";
  render();
}

async function toggleRecording() {
  if (tauriInvoke) {
    try {
      const status = await tauriInvoke<DesktopStatus>(state.captureActive ? "stop_capture" : "start_capture");
      applyDesktopStatus(status);
      await refreshClips();
      render();
      return;
    } catch (error) {
      console.error("Could not toggle desktop recording", error);
    }
  }

  state.sessionRecording = !state.sessionRecording;
  state.recordingState = state.sessionRecording ? "RecordingSession" : "Buffering";
  render();
}

function applyDesktopStatus(status: DesktopStatus) {
  state.recordingState = status.recording_state;
  state.detectedGame = status.detected_game ?? "Waiting for game";
  state.replayBufferSeconds = status.replay_buffer_seconds;
  state.micEnabled = status.mic_enabled;
  state.micDevice = status.mic_device ?? "";
  state.autoRecordEnabled = status.auto_record_enabled;
  state.autoUpload = status.upload_enabled;
  state.sessionRecording = status.session_recording;
  state.captureActive = status.capture_active;
  state.capturePath = status.capture_path;
  state.ffmpegAvailable = status.ffmpeg_available;
  state.ffmpegPath = status.ffmpeg_path;
  state.systemAudioAvailable = status.system_audio_available;
  state.desktopDuplicationAvailable = status.desktop_duplication_available;
}

async function trimSelectedClip() {
  if (!tauriInvoke) {
    return;
  }

  const clip = state.clips.find((candidate) => candidate.id === state.selectedClipId);
  if (!clip) {
    return;
  }

  const startInput = appRoot.querySelector<HTMLInputElement>("[data-action='trim-start']");
  const endInput = appRoot.querySelector<HTMLInputElement>("[data-action='trim-end']");
  const startSeconds = parseTimestamp(startInput?.value ?? "0");
  const endSeconds = parseTimestamp(endInput?.value ?? clip.duration);

  try {
    const dto = await tauriInvoke<ClipDto>("trim_clip", {
      clipId: clip.id,
      startSeconds,
      endSeconds,
    });
    const trimmed = clipFromDto(dto);
    state.clips.unshift(trimmed);
    state.selectedClipId = trimmed.id;
    await refreshClips();
    render();
  } catch (error) {
    console.error("Could not trim clip", error);
  }
}

async function revealSelectedClip() {
  if (!tauriInvoke || !state.selectedClipId) {
    return;
  }

  try {
    await tauriInvoke("reveal_clip", { clipId: state.selectedClipId });
  } catch (error) {
    console.error("Could not reveal clip", error);
  }
}

async function uploadSelectedClip() {
  if (!tauriInvoke || !state.selectedClipId) {
    return;
  }

  try {
    const dto = await tauriInvoke<ClipDto>("upload_clip", {
      clipId: state.selectedClipId,
      provider: state.uploadProvider,
      customEndpoint: state.customUploadEndpoint,
      customResponseUrlPath: state.customUploadResponsePath,
    });
    const updated = clipFromDto(dto);
    state.clips = state.clips.map((clip) => (clip.id === updated.id ? updated : clip));
    render();
  } catch (error) {
    console.error("Could not queue upload", error);
  }
}

async function writeGsiConfigs() {
  if (!tauriInvoke) {
    return;
  }

  try {
    const configs = await tauriInvoke<GsiConfigDto[]>("write_gsi_configs");
    state.gsiConfigStatus = configs.map((config) => `${config.game_id}: ${config.path}`).join(" · ");
    render();
  } catch (error) {
    state.gsiConfigStatus = `Could not write GSI configs: ${String(error)}`;
    render();
  }
}

async function deleteSelectedClip() {
  if (!tauriInvoke || !state.selectedClipId) {
    return;
  }

  try {
    const clips = await tauriInvoke<ClipDto[]>("delete_clip", { clipId: state.selectedClipId });
    state.clips = clips.map(clipFromDto);
    state.selectedClipId = state.clips[0]?.id ?? "";
    render();
  } catch (error) {
    console.error("Could not delete clip", error);
  }
}

async function refreshClips() {
  if (!tauriInvoke) {
    return;
  }

  const clips = await tauriInvoke<ClipDto[]>("list_clips");
  state.clips = clips.map(clipFromDto);
  if (!state.clips.some((clip) => clip.id === state.selectedClipId)) {
    state.selectedClipId = state.clips[0]?.id ?? "";
  }
}

async function saveReplayBufferSetting(seconds: number) {
  if (!tauriInvoke) {
    return;
  }

  try {
    const status = await tauriInvoke<DesktopStatus>("set_replay_buffer", { seconds });
    applyDesktopStatus(status);
    render();
  } catch (error) {
    console.warn("Could not save replay buffer setting", error);
  }
}

async function saveMicSetting(enabled: boolean) {
  if (!tauriInvoke) {
    return;
  }

  try {
    const status = await tauriInvoke<DesktopStatus>("set_mic_enabled", { enabled });
    applyDesktopStatus(status);
    render();
  } catch (error) {
    console.warn("Could not save mic setting", error);
  }
}

async function saveMicDevice(device: string) {
  if (!tauriInvoke) {
    return;
  }

  try {
    const status = await tauriInvoke<DesktopStatus>("set_mic_device", { device: device || null });
    applyDesktopStatus(status);
    render();
  } catch (error) {
    console.warn("Could not save mic device", error);
  }
}

async function saveAutoRecordSetting(enabled: boolean) {
  if (!tauriInvoke) {
    return;
  }

  try {
    const status = await tauriInvoke<DesktopStatus>("set_auto_record_enabled", { enabled });
    applyDesktopStatus(status);
    render();
  } catch (error) {
    console.warn("Could not save auto-record setting", error);
  }
}

async function refreshStatus() {
  if (!tauriInvoke) {
    return;
  }

  try {
    const status = await tauriInvoke<DesktopStatus>("refresh_detected_game");
    applyDesktopStatus(status);
    render();
  } catch (error) {
    console.warn("Could not refresh detected game", error);
  }
}

async function refreshAudioDevices() {
  if (!tauriInvoke) {
    return;
  }

  try {
    state.audioDevices = await tauriInvoke<AudioDeviceDto[]>("list_audio_devices");
    render();
  } catch (error) {
    console.warn("Could not load audio devices", error);
  }
}

async function pollAutoClips() {
  if (!tauriInvoke || !state.captureActive) {
    return;
  }

  try {
    const clips = await tauriInvoke<ClipDto[]>("poll_auto_clip_events");
    if (clips.length === 0) {
      return;
    }
    const created = clips.map(clipFromDto);
    state.clips = [...created, ...state.clips.filter((clip) => !created.some((fresh) => fresh.id === clip.id))];
    state.selectedClipId = created[0].id;
    state.activeView = "Library";
    render();
  } catch (error) {
    console.warn("Could not poll auto clips", error);
  }
}

function parseTimestamp(value: string) {
  const parts = value.trim().split(":").map(Number);
  if (parts.some((part) => Number.isNaN(part))) {
    return 0;
  }
  if (parts.length === 1) {
    return parts[0];
  }
  if (parts.length === 2) {
    return parts[0] * 60 + parts[1];
  }
  return parts[0] * 3600 + parts[1] * 60 + parts[2];
}

function escapeHtml(value: string) {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

async function loadDesktopBridge() {
  const hasTauri = "__TAURI_INTERNALS__" in window;
  if (!hasTauri) {
    return;
  }

  try {
    tauriInvoke = window.__TAURI__?.core?.invoke ?? null;
    if (!tauriInvoke) {
      return;
    }
    const invoke = tauriInvoke;
    const status = await invoke<DesktopStatus>("get_status");
    applyDesktopStatus(status);
    const clips = await invoke<ClipDto[]>("list_clips");
    state.clips = clips.map(clipFromDto);
    state.selectedClipId = state.clips[0]?.id ?? "";
    render();
    await refreshAudioDevices();
    window.setInterval(() => {
      void refreshStatus();
    }, 3_000);
    window.setInterval(() => {
      void pollAutoClips();
    }, 2_000);
  } catch (error) {
    console.warn("Tauri bridge unavailable", error);
  }
}

render();
void loadDesktopBridge();





