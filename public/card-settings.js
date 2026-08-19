import { html, useEffect, useState } from "/public/vendor/htm-preact.js";
import { Card } from "/public/card.js";

export const CARD_SETTING_KEYS = {
  cardSize: "table-card-size-percent",
  rankSize: "table-rank-size-percent",
  rankWeight: "table-rank-weight-percent",
  fourColor: "table-four-color-suits",
  paranoid: "table-paranoid-cards",
};

const DEFAULT_CARD_SCALE = 1.8;

function savedSetting(key) {
  const value = Number(localStorage.getItem(key));
  return value >= 50 && value <= 200 ? value : 100;
}

export function readCardSettings() {
  return {
    cardSize: savedSetting(CARD_SETTING_KEYS.cardSize),
    rankSize: savedSetting(CARD_SETTING_KEYS.rankSize),
    rankWeight: savedSetting(CARD_SETTING_KEYS.rankWeight),
    fourColor: localStorage.getItem(CARD_SETTING_KEYS.fourColor) === "on",
    paranoid: localStorage.getItem(CARD_SETTING_KEYS.paranoid) === "on",
  };
}

function rankWeight(percent) {
  return Math.round(percent <= 100 ? 400 + (percent - 50) * 6 : 700 + (percent - 100) * 2);
}

function rankStroke(percent) {
  return `${Math.max(0, (percent - 100) * 0.00045).toFixed(3)}em`;
}

export function applyCardSettings({ cardSize, rankSize, rankWeight: rankBoldness, fourColor }) {
  const scale = DEFAULT_CARD_SCALE * cardSize / 100;
  const root = document.documentElement;
  root.classList.toggle("four-color-suits", Boolean(fourColor));
  root.style.setProperty("--card-size-scale", String(cardSize / 100));
  root.style.setProperty("--card-rank-percent", String(rankSize / 100));
  root.style.setProperty("--card-rank-weight", String(rankWeight(rankBoldness)));
  root.style.setProperty("--card-rank-stroke", rankStroke(rankBoldness));
  root.style.setProperty("--viewer-card-scale", String(DEFAULT_CARD_SCALE * cardSize));
  root.style.setProperty("--viewer-card-w", `${3 * scale}rem`);
  root.style.setProperty("--viewer-card-h", `${4.2 * scale}rem`);
  root.style.setProperty("--viewer-card-w-mobile", `${2.1 * scale}rem`);
  root.style.setProperty("--viewer-card-h-mobile", `${2.95 * scale}rem`);
  root.style.setProperty("--viewer-stage-extra", `${Math.max(0, 6.2 * (scale - DEFAULT_CARD_SCALE))}rem`);
}

export function useCardSettings() {
  const [settings, setSettings] = useState(readCardSettings);
  useEffect(() => applyCardSettings(settings), [settings]);
  return [settings, setSettings];
}

export function CardSettings({ settings: providedSettings, setSettings: providedSetSettings, interactive = false, concealable = false, trigger = true } = {}) {
  const [localSettings, localSetSettings] = useCardSettings();
  const settings = providedSettings || localSettings;
  const setSettings = providedSetSettings || localSetSettings;
  const update = (name, key) => (event) => {
    const value = Number(event.currentTarget.value);
    setSettings((current) => ({ ...current, [name]: value }));
    localStorage.setItem(key, String(value));
  };
  const toggle = (name, key) => (event) => {
    const value = event.currentTarget.checked;
    setSettings((current) => ({ ...current, [name]: value }));
    localStorage.setItem(key, value ? "on" : "off");
  };
  const dialog = html`<dialog id="card-config" class="card-config-dialog">
      <form method="dialog">
        <header><h2>Card display</h2><button type="submit" title="Close" aria-label="Close">×</button></header>
        <div class="card-config-preview" aria-label="Card preview"><${Card} card="5c" interactive=${interactive} /><${Card} card="6c" interactive=${interactive} /></div>
        <label><span>Card size <output>${settings.cardSize}%</output></span><input name="card-scale" type="range" min="50" max="200" step="5" value=${settings.cardSize} onInput=${update("cardSize", CARD_SETTING_KEYS.cardSize)} /></label>
        <label><span>Rank size <output>${settings.rankSize}%</output></span><input name="rank-scale" type="range" min="50" max="200" step="5" value=${settings.rankSize} onInput=${update("rankSize", CARD_SETTING_KEYS.rankSize)} /></label>
        <label><span>Rank weight <output>${settings.rankWeight}%</output></span><input name="rank-weight" type="range" min="50" max="200" step="5" value=${settings.rankWeight} onInput=${update("rankWeight", CARD_SETTING_KEYS.rankWeight)} /></label>
        <label class="card-option-toggle"><input name="four-color" type="checkbox" checked=${settings.fourColor} onChange=${toggle("fourColor", CARD_SETTING_KEYS.fourColor)} /><span><b>Four-color suits</b><small>Color clubs blue and diamonds orange</small></span></label>
        ${concealable && html`<label class="card-option-toggle"><input name="paranoid" type="checkbox" checked=${settings.paranoid} onChange=${toggle("paranoid", CARD_SETTING_KEYS.paranoid)} /><span><b>Paranoid mode</b><small>Keep your hole cards face down until you hover or hold them</small></span></label>`}
      </form>
    </dialog>`;
  return trigger
    ? html`<div class="card-settings"><button class="table-config-button" type="button" title="Card display settings" aria-label="Card display settings" onClick=${() => document.getElementById("card-config")?.showModal()}>⚙</button>${dialog}</div>`
    : dialog;
}
