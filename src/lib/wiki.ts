import { openUrl } from "@tauri-apps/plugin-opener";
import { WARFRAME_WIKI_BASE } from "../constants/urls";

export function wikiUrl(name: string) {
  return `${WARFRAME_WIKI_BASE}/Special:Search?search=${encodeURIComponent(name)}`;
}

export function openWiki(name: string) {
  openUrl(wikiUrl(name));
}

export async function copyWikiLink(name: string) {
  try {
    await navigator.clipboard.writeText(wikiUrl(name));
  } catch {
    const ta = document.createElement("textarea");
    ta.value = wikiUrl(name);
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    document.body.removeChild(ta);
  }
}
