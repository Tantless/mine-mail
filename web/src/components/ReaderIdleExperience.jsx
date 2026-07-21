import { useEffect, useState } from "react";

export const READER_IDLE_SCENE_DURATION_MS = 9000;

export const READER_IDLE_SCENES = Object.freeze([
  Object.freeze({
    effect: "char-rise",
    lines: Object.freeze(["海内存知己，", "天涯若比邻。"]),
    source: "王勃 · 送杜少府之任蜀州",
  }),
  Object.freeze({
    effect: "char-fly",
    lines: Object.freeze(["云中谁寄锦书来？"]),
    source: "李清照 · 一剪梅",
  }),
  Object.freeze({
    effect: "char-focus",
    lines: Object.freeze(["驿寄梅花，", "鱼传尺素。"]),
    source: "秦观 · 踏莎行",
  }),
  Object.freeze({
    effect: "char-depth",
    lines: Object.freeze(["江水三千里，", "家书十五行。"]),
    source: "袁凯 · 京师得家书",
  }),
]);

function motionPreference() {
  return typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-reduced-motion: reduce)").matches
    : false;
}

function pageIsVisible() {
  return !document.hidden;
}

export function ReaderIdleExperience() {
  const [sceneIndex, setSceneIndex] = useState(0);
  const [prefersReducedMotion, setPrefersReducedMotion] = useState(
    motionPreference,
  );
  const [isVisible, setIsVisible] = useState(pageIsVisible);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return undefined;
    const mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    const handlePreferenceChange = (event) => {
      setPrefersReducedMotion(event.matches);
    };

    setPrefersReducedMotion(mediaQuery.matches);
    if (typeof mediaQuery.addEventListener === "function") {
      mediaQuery.addEventListener("change", handlePreferenceChange);
      return () => mediaQuery.removeEventListener("change", handlePreferenceChange);
    }

    mediaQuery.addListener?.(handlePreferenceChange);
    return () => mediaQuery.removeListener?.(handlePreferenceChange);
  }, []);

  useEffect(() => {
    const handleVisibilityChange = () => setIsVisible(pageIsVisible());
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () =>
      document.removeEventListener("visibilitychange", handleVisibilityChange);
  }, []);

  useEffect(() => {
    if (prefersReducedMotion) {
      setSceneIndex(0);
      return undefined;
    }
    if (!isVisible) return undefined;

    const timer = window.setTimeout(() => {
      setSceneIndex((current) => (current + 1) % READER_IDLE_SCENES.length);
    }, READER_IDLE_SCENE_DURATION_MS);
    return () => window.clearTimeout(timer);
  }, [isVisible, prefersReducedMotion, sceneIndex]);

  const scene = READER_IDLE_SCENES[sceneIndex];
  const lineLengths = scene.lines.map((line) => Array.from(line).length);
  const isBalancedCouplet =
    lineLengths.length === 2 && lineLengths[0] === lineLengths[1];
  const widestLineLength = Math.max(...lineLengths);
  const compositionUnits =
    widestLineLength * 1.08 + (isBalancedCouplet ? 4 : 0);
  const characterCount = scene.lines.reduce(
    (total, line) => total + Array.from(line).length,
    0,
  );
  let characterOffset = 0;

  return (
    <div
      className="reader-idle"
      aria-hidden="true"
      data-paused={isVisible ? undefined : "true"}
      data-reduced-motion={prefersReducedMotion ? "true" : undefined}
      data-scene={sceneIndex}
    >
      <figure
        key={`${sceneIndex}-${scene.effect}`}
        className="reader-idle__figure"
        data-effect={scene.effect}
        style={{
          "--reader-idle-composition-units": compositionUnits.toFixed(2),
          "--source-delay": `${characterCount * 120 + 820}ms`,
        }}
      >
        <blockquote
          className="reader-idle__quote"
          data-staggered={isBalancedCouplet ? "true" : undefined}
        >
          {scene.lines.map((line, lineIndex) => {
            const characters = Array.from(line);
            const lineStart = characterOffset;
            characterOffset += characters.length;

            return (
              <span
                className="reader-idle__line"
                key={`${scene.effect}-${lineIndex}`}
              >
                {characters.map((character, characterIndex) => (
                  <span
                    className="reader-idle__char"
                    key={`${characterIndex}-${character}`}
                    style={{ "--char-index": lineStart + characterIndex }}
                  >
                    {character}
                  </span>
                ))}
              </span>
            );
          })}
        </blockquote>
        <figcaption className="reader-idle__source">——{scene.source}</figcaption>
      </figure>
    </div>
  );
}
