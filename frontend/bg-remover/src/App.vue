<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";

type BgMode = "transparent" | "color" | "image";

const IMAGE_EXT = ["png", "jpg", "jpeg", "webp"];
const MIME: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  webp: "image/webp",
};
const SWATCHES = ["#ffffff", "#000000", "#e74c3c", "#3498db", "#2ecc71"];

const fileName = ref("");
const originalUrl = ref<string | null>(null);
const resultUrl = ref<string | null>(null);
const showOriginal = ref(false);
const busy = ref(false);
const error = ref("");

const bgMode = ref<BgMode>("transparent");
const bgColor = ref("#ffffff");
const bgImagePath = ref<string | null>(null);
const bgImageUrl = ref<string | null>(null);

function extOf(path: string) {
  return path.split(".").pop()?.toLowerCase() ?? "";
}

async function fileToUrl(path: string): Promise<string> {
  const bytes = await invoke<ArrayBuffer>("load_file", { path });
  const type = MIME[extOf(path)] ?? "image/png";
  return URL.createObjectURL(new Blob([bytes], { type }));
}

function revoke(url: string | null) {
  if (url) URL.revokeObjectURL(url);
}

async function processFile(path: string) {
  if (!IMAGE_EXT.includes(extOf(path))) {
    error.value = "Please use a PNG, JPG or WebP image.";
    return;
  }
  error.value = "";
  showOriginal.value = false;
  fileName.value = path.split(/[\\/]/).pop() ?? path;
  busy.value = true;

  revoke(resultUrl.value);
  resultUrl.value = null;

  try {
    revoke(originalUrl.value);
    originalUrl.value = await fileToUrl(path);

    const bytes = await invoke<ArrayBuffer>("remove_background", { path });
    resultUrl.value = URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}

async function chooseImage() {
  const selected = await open({
    multiple: false,
    filters: [{ name: "Images", extensions: IMAGE_EXT }],
  });
  if (selected) await processFile(selected);
}

async function chooseBackgroundImage() {
  const selected = await open({
    multiple: false,
    filters: [{ name: "Images", extensions: IMAGE_EXT }],
  });
  if (!selected) return;
  try {
    revoke(bgImageUrl.value);
    bgImageUrl.value = await fileToUrl(selected);
    bgImagePath.value = selected;
    bgMode.value = "image";
  } catch (e) {
    error.value = String(e);
  }
}

function pickSwatch(color: string) {
  bgColor.value = color;
  bgMode.value = "color";
}

// What gets sent to Rust when saving. Matches what the preview shows.
function currentBackground() {
  if (bgMode.value === "color") return { kind: "color", hex: bgColor.value };
  if (bgMode.value === "image" && bgImagePath.value)
    return { kind: "image", path: bgImagePath.value };
  return { kind: "transparent" };
}

async function saveImage() {
  const dest = await save({
    defaultPath: "result.png",
    filters: [{ name: "PNG", extensions: ["png"] }],
  });
  if (!dest) return;
  try {
    await invoke("save_result", { path: dest, background: currentBackground() });
  } catch (e) {
    error.value = String(e);
  }
}

const shownUrl = computed(() => (showOriginal.value ? originalUrl.value : resultUrl.value));

const checkerboard = computed(
  () =>
    !showOriginal.value &&
    (bgMode.value === "transparent" ||
      (bgMode.value === "image" && !bgImageUrl.value))
);

const frameStyle = computed(() => {
  if (showOriginal.value) return {};
  if (bgMode.value === "color") return { backgroundColor: bgColor.value };
  if (bgMode.value === "image" && bgImageUrl.value)
    return {
      backgroundImage: `url(${bgImageUrl.value})`,
      backgroundSize: "cover",
      backgroundPosition: "center",
    };
  return {};
});

// Drag and drop
let unlisten: (() => void) | null = null;
onMounted(async () => {
  unlisten = await getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "drop" && !busy.value) {
      const path = event.payload.paths[0];
      if (path) processFile(path);
    }
  });
});
onBeforeUnmount(() => unlisten?.());
</script>

<template>
  <main class="app">
    <h1>Background Remover</h1>

    <div class="row">
      <button :disabled="busy" @click="chooseImage">
        {{ busy ? "Working..." : "Choose image" }}
      </button>
      <button :disabled="!resultUrl || busy" @click="saveImage">Save PNG</button>
      <button :disabled="!resultUrl || busy" @click="showOriginal = !showOriginal">
        {{ showOriginal ? "Show result" : "Show original" }}
      </button>
    </div>

    <div class="row bg">
      <span>Background:</span>
      <label><input type="radio" value="transparent" v-model="bgMode" /> Transparent</label>
      <label><input type="radio" value="color" v-model="bgMode" /> Color</label>
      <label><input type="radio" value="image" v-model="bgMode" /> Image</label>
    </div>

    <div class="row">
      <button
        v-for="c in SWATCHES"
        :key="c"
        class="swatch"
        :style="{ backgroundColor: c }"
        :title="c"
        @click="pickSwatch(c)"
      ></button>
      <input type="color" v-model="bgColor" @input="bgMode = 'color'" />
      <button @click="chooseBackgroundImage">Choose background image</button>
    </div>

    <p v-if="fileName" class="name">{{ fileName }}</p>
    <p v-if="error" class="error">{{ error }}</p>

    <div class="preview">
      <div v-if="shownUrl" class="frame" :class="{ checker: checkerboard }" :style="frameStyle">
        <img :src="shownUrl" alt="Preview" />
      </div>
      <p v-else-if="busy">Removing background, this can take a few seconds...</p>
      <p v-else>Choose an image or drag one onto this window.</p>
    </div>
  </main>
</template>

<style>
body { margin: 0; font-family: system-ui, sans-serif; }
.app { max-width: 900px; margin: 0 auto; padding: 24px; text-align: center; }
.row { display: flex; gap: 10px; justify-content: center; align-items: center; margin: 12px 0; flex-wrap: wrap; }
button { padding: 10px 18px; font-size: 1rem; cursor: pointer; }
.swatch { width: 32px; height: 32px; padding: 0; border: 2px solid #888; border-radius: 50%; }
.name { color: #666; margin: 0; }
.error { color: #c0392b; white-space: pre-wrap; }
.preview {
  margin-top: 16px; min-height: 300px; display: flex;
  align-items: center; justify-content: center;
  border-radius: 8px; background: #f0f0f0;
}
.frame { display: inline-block; line-height: 0; }
.frame img { display: block; max-width: min(840px, calc(100vw - 80px)); max-height: 65vh; }
.checker {
  background-color: #ddd;
  background-image:
    linear-gradient(45deg, #bbb 25%, transparent 25%, transparent 75%, #bbb 75%),
    linear-gradient(45deg, #bbb 25%, transparent 25%, transparent 75%, #bbb 75%);
  background-size: 20px 20px;
  background-position: 0 0, 10px 10px;
}
</style>