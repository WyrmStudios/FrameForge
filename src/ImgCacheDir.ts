import { createContext } from "react";

/** Absolute path to the local image cache directory (%LOCALAPPDATA%\warframe-companion\img_cache).
 *  Empty string = not yet known (images fall back to CDN). Set once on app startup. */
export const ImgCacheDirContext = createContext<string>("");
