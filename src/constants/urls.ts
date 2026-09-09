export const WARFRAMESTAT_IMAGE_CDN = "https://cdn.warframestat.us/img";
export const WARFRAME_WIKI_BASE = "https://wiki.warframe.com/w";

export function warframeStatImageUrl(imageName: string): string {
  return `${WARFRAMESTAT_IMAGE_CDN}/${imageName}`;
}
