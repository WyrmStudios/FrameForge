import { useContext, useState, useRef, useEffect } from "react";
import { ImgCacheDirContext } from "./ImgCacheDir";
import { warframeStatImageUrl } from "./constants/urls";

function BlueprintIcon() {
  return (
    <svg className="img-fallback" viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg">
      <rect x="5" y="2" width="17" height="22" rx="1.5" fill="#0d1f33" stroke="#388bfd" strokeWidth="1.2"/>
      <path d="M18 2 L22 6 L18 6 Z" fill="#388bfd" opacity="0.5"/>
      <line x1="8" y1="11" x2="19" y2="11" stroke="#388bfd" strokeWidth="1" opacity="0.9"/>
      <line x1="8" y1="14" x2="19" y2="14" stroke="#388bfd" strokeWidth="1" opacity="0.9"/>
      <line x1="8" y1="17" x2="14" y2="17" stroke="#388bfd" strokeWidth="1" opacity="0.9"/>
      <circle cx="23" cy="23" r="6" fill="#0d1117" stroke="#388bfd" strokeWidth="1.2"/>
      <line x1="23" y1="20" x2="23" y2="26" stroke="#388bfd" strokeWidth="1.2"/>
      <line x1="20" y1="23" x2="26" y2="23" stroke="#388bfd" strokeWidth="1.2"/>
    </svg>
  );
}

export default function ItemImg({ imageName, category, size = 32 }: { imageName?: string; category: string; size?: number }) {
  const baseUrl = useContext(ImgCacheDirContext);
  const [localFailed, setLocalFailed] = useState(false);
  const [failed, setFailed] = useState(false);
  const ref = useRef<HTMLImageElement>(null);
  const style = { width: size, height: size, flexShrink: 0 as const };

  useEffect(() => {
    if (ref.current?.complete) ref.current.classList.add("img-loaded");
  }, []);

  if (!imageName || failed) {
    if (category === "Blueprints") return <BlueprintIcon />;
    return <span className="img-fallback" style={{ ...style, fontSize: size * 0.35 }}>{category[0].toUpperCase()}</span>;
  }
  if (imageName.startsWith("http") || imageName.startsWith("/")) {
    return <img ref={ref} className="img" style={style} src={imageName} alt="" loading="lazy" onError={() => setFailed(true)} onLoad={() => ref.current?.classList.add("img-loaded")} />;
  }
  const useLocal = Boolean(baseUrl) && !localFailed;
  const src = useLocal ? `${baseUrl}/${imageName}` : warframeStatImageUrl(imageName);
  return (
    <img ref={ref} className="img" style={style} src={src} alt="" loading="lazy"
      onError={() => useLocal ? setLocalFailed(true) : setFailed(true)} onLoad={() => ref.current?.classList.add("img-loaded")} />
  );
}
