<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

const fileName = ref("");
const resultUrl = ref<string | null>(null);
const busy = ref(false);
const error = ref("");

async function chooseImage() {
  error.value = "";
  const selected = await open({
    multiple: false,
    filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
  });
  if (!selected) return;

  fileName.value = selected.split(/[\\/]/).pop() ?? selected;
  busy.value = true;
  try {
    const bytes = await invoke<ArrayBuffer>("remove_background", { path: selected });
    if (resultUrl.value) URL.revokeObjectURL(resultUrl.value);
    resultUrl.value = URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}

async function saveImage() {
  const dest = await save({
    defaultPath: "result.png",
    filters: [{ name: "PNG", extensions: ["png"] }],
  });
  if (!dest) return;
  try {
    await invoke("save_result", { path: dest });
  } catch (e) {
    error.value = String(e);
  }
}
</script>

<template>
  <main class="app">
    <h1>Background Remover</h1>

    <div class="buttons">
      <button :disabled="busy" @click="chooseImage">
        {{ busy ? "Working..." : "Choose image" }}
      </button>
      <button :disabled="!resultUrl || busy" @click="saveImage">Save PNG</button>
    </div>

    <p v-if="fileName" class="name">{{ fileName }}</p>
    <p v-if="error" class="error">{{ error }}</p>

    <div class="preview">
      <img v-if="resultUrl" :src="resultUrl" alt="Result" />
      <p v-else-if="busy">Removing background, this can take a few seconds...</p>
      <p v-else>Pick an image to start.</p>
    </div>
  </main>
</template>

<style>
body { margin: 0; font-family: system-ui, sans-serif; }
.app { max-width: 900px; margin: 0 auto; padding: 24px; text-align: center; }
.buttons { display: flex; gap: 12px; justify-content: center; margin: 16px 0; }
button { padding: 10px 20px; font-size: 1rem; cursor: pointer; }
.name { color: #666; margin: 0; }
.error { color: #c0392b; white-space: pre-wrap; }
.preview {
  margin-top: 16px; min-height: 300px; display: flex;
  align-items: center; justify-content: center; border-radius: 8px;
  background-color: #ddd;
  background-image:
    linear-gradient(45deg, #bbb 25%, transparent 25%, transparent 75%, #bbb 75%),
    linear-gradient(45deg, #bbb 25%, transparent 25%, transparent 75%, #bbb 75%);
  background-size: 20px 20px;
  background-position: 0 0, 10px 10px;
}
.preview img { max-width: 100%; max-height: 70vh; }
</style>