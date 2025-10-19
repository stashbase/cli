<template>
  <div class="container">
    <h2>Stashbase scanner test</h2>

    <!-- secret embedded inside a comment (template-level) -->
    <!-- NOTE(secret): s3cr3t_TEST_12345 -->

    <!-- secret split across data attributes -->
    <div
      id="hidden-parts"
      data-p1="s3cr"
      data-p2="3t_TE"
      data-p3="ST_12345"
      style="display: none"
    ></div>

    <!-- secret encoded as base64 inside an attribute -->
    <img alt="invisible" :data-b64="b64Secret" style="display: none" />

    <!-- secret embedded as CSS custom property -->
    <div class="css-secret" style="display: none" data-marker>hidden</div>

    <!-- JSON blob containing secret inside script tag (non-executing) -->
    <script type="application/json" id="json-secret">
      {
        "meta": "info",
        "auth": "s3cr3t_TEST_12345",
        "notes": "for testing only"
      }
    </script>

    <!-- secret split into char codes (array) -->
    <div style="display: none" id="charcodes"></div>

    <!-- reveal button reconstructs (for local testing) -->
    <button @click="reveal">Reveal (dev only)</button>

    <pre v-if="revealed">{{ revealed }}</pre>
  </div>
</template>

<script setup>
import { ref, onMounted } from "vue";

const revealed = ref(null);

// base64 of "s3cr3t_TEST_12345"
const b64Secret = "czNjcjN0X1RFU1RfMTIzNDU=";

// char codes array (for s3cr3t_TEST_12345)
const charCodes = [
  115, 51, 99, 114, 51, 116, 95, 84, 69, 83, 84, 95, 49, 50, 51, 52, 53,
];

// small "obfuscation" helpers
function fromCharCodes(arr) {
  return String.fromCharCode(...arr);
}
function joinParts(...parts) {
  return parts.join("");
}
function fromB64(s) {
  try {
    return atob(s);
  } catch {
    return null;
  }
}

onMounted(() => {
  // store a JSON blob in localStorage (hidden)
  try {
    localStorage.setItem(
      "__app_meta__",
      JSON.stringify({ token: "s3cr3t_TEST_12345", created: Date.now() })
    );
  } catch (e) {
    /* ignore */
  }

  // also create a pseudo hidden meta element constructed programmatically
  const el = document.createElement("meta");
  el.name = "x-hidden";
  // place secret inside an attribute but obfuscated slightly
  el.content = "s3cr3t" + "_" + ["TEST", "12345"].join("_"); // s3cr3t_TEST_12345
  document.head.appendChild(el);
});

function reconstructFromDOM() {
  // 1) read split data attributes
  const hp = document.getElementById("hidden-parts");
  const p = hp
    ? joinParts(hp.dataset.p1 || "", hp.dataset.p2 || "", hp.dataset.p3 || "")
    : null;

  // 2) read base64 attribute
  const img = document.querySelector('img[alt="invisible"]');
  const b64 = img?.dataset?.b64 ? fromB64(img.dataset.b64) : null;

  // 3) read JSON script
  const jsonEl = document.getElementById("json-secret");
  let jsonSecret = null;
  if (jsonEl) {
    try {
      const parsed = JSON.parse(jsonEl.textContent || "{}");
      jsonSecret = parsed.auth;
    } catch {}
  }

  // 4) char codes
  const cc = fromCharCodes(charCodes);

  // 5) css custom property (stored in .css-secret: --secret)
  let cssSecret = null;
  const cssEl = document.querySelector(".css-secret");
  if (cssEl) {
    cssSecret =
      getComputedStyle(cssEl).getPropertyValue("--secret").trim() || null;
  }

  // 6) meta tag added in onMounted
  const meta = document.querySelector('meta[name="x-hidden"]');
  const metaSecret = meta?.content || null;

  // 7) localStorage fallback
  let ls = null;
  try {
    const maybe = JSON.parse(localStorage.getItem("__app_meta__") || "{}");
    ls = maybe.token;
  } catch {}

  // combine best candidates (dedupe)
  const candidates = [p, b64, jsonSecret, cc, cssSecret, metaSecret, ls].filter(
    Boolean
  );
  return Array.from(new Set(candidates)).join(" | ");
}

function reveal() {
  const found = reconstructFromDOM();
  revealed.value = found || "no secret found";
}
</script>

<style>
.container {
  padding: 1rem;
  font-family: system-ui, sans-serif;
}
.css-secret {
  /* secret placed into a CSS custom property (non-obvious) */
  --secret: s3cr3t_TEST_12345;
  /* also placed in a pseudo-element content for scanners that parse styles */
}
.css-secret::after {
  content: var(--secret);
  display: none;
}
</style>
