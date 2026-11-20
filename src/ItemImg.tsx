import { useContext, useState, useRef, useEffect } from "react";
import { ImgCacheDirContext } from "./ImgCacheDir";
import { warframeStatImageUrl } from "./constants/urls";

function BlueprintIcon() {
  return (
    <svg className="img-fallback size-8 shrink-0 rounded-4 border-0 bg-transparent p-0" viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg">
      <rect x="5" y="2" width="17" height="22" rx="1.5" fill="#0d1f33" strokeWidth="1.2" style={{ stroke: "var(--color-accent)" }}/>
      <path d="M18 2 L22 6 L18 6 Z" opacity="0.5" style={{ fill: "var(--color-accent)" }}/>
      <line x1="8" y1="11" x2="19" y2="11" strokeWidth="1" opacity="0.9" style={{ stroke: "var(--color-accent)" }}/>
      <line x1="8" y1="14" x2="19" y2="14" strokeWidth="1" opacity="0.9" style={{ stroke: "var(--color-accent)" }}/>
      <line x1="8" y1="17" x2="14" y2="17" strokeWidth="1" opacity="0.9" style={{ stroke: "var(--color-accent)" }}/>
      <circle cx="23" cy="23" r="6" strokeWidth="1.2" style={{ fill: "var(--color-background)", stroke: "var(--color-accent)" }}/>
      <line x1="23" y1="20" x2="23" y2="26" strokeWidth="1.2" style={{ stroke: "var(--color-accent)" }}/>
      <line x1="20" y1="23" x2="26" y2="23" strokeWidth="1.2" style={{ stroke: "var(--color-accent)" }}/>
    </svg>
  );
}

interface Props {
  imageName?: string;
  category?: string;
  size?: number;
  className?: string;
  fallbackClassName?: string;
  fallbackText?: string;
}

export default function ItemImg({ imageName, category = "?", size = 32, className = "img", fallbackClassName = "img-fallback", fallbackText }: Props) {
  const baseUrl = useContext(ImgCacheDirContext);
  const [localFailed, setLocalFailed] = useState(false);
  const [failed, setFailed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [retryCount, setRetryCount] = useState(0);
  const ref = useRef<HTMLImageElement>(null);
  const retryTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const imageStyle = { width: size, height: size, flexShrink: 0 as const };

  useEffect(() => {
    setLocalFailed(false);
    setFailed(false);
    setLoaded(false);
    setRetryCount(0);
    if (ref.current?.complete) setLoaded(true);
    return () => {
      if (retryTimer.current) clearTimeout(retryTimer.current);
    };
  }, [imageName]);

  const imageClassName = className === "img"
    ? `${className} shrink-0 rounded-4 object-contain opacity-0 -translate-x-1 -translate-y-1 scale-90 transition-[opacity,transform] duration-250 ease-out${loaded ? " translate-x-0 translate-y-0 scale-100 opacity-100" : ""}`
    : className;
  const fallbackClassNames = fallbackClassName === "img-fallback"
    ? `${fallbackClassName} flex shrink-0 items-center justify-center rounded-4 border border-border bg-white/6 font-semibold text-muted`
    : fallbackClassName;

  if (!imageName || failed) {
    if (category === "Blueprints") return <BlueprintIcon />;
    return <span className={fallbackClassNames} style={{ ...imageStyle, fontSize: size * 0.35 }}>{fallbackText ?? category[0].toUpperCase()}</span>;
  }
  if (imageName.startsWith("http") || imageName.startsWith("/")) {
    return <img ref={ref} className={imageClassName} style={imageStyle} src={imageName} alt="" loading="lazy" onError={() => setFailed(true)} onLoad={() => setLoaded(true)} />;
  }
  const useLocal = Boolean(baseUrl) && !localFailed;
  const src = useLocal ? `${baseUrl}/${imageName}` : warframeStatImageUrl(imageName);
  return (
    <img key={`${imageName}:${useLocal ? "local" : "cdn"}:${retryCount}`} ref={ref} className={imageClassName} style={imageStyle} src={src} alt="" loading="lazy"
      onError={() => {
        if (useLocal) {
          setLocalFailed(true);
        } else if (retryCount < 2) {
          retryTimer.current = setTimeout(() => setRetryCount(count => count + 1), 500 * (retryCount + 1));
        } else {
          setFailed(true);
        }
      }} onLoad={() => setLoaded(true)} />
  );
}
