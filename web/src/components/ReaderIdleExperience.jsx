import { useEffect, useState } from "react";

export const READER_IDLE_SCENE_DURATION_MS = 9000;

const quote = (id, text, source) => Object.freeze({ id, text, source });

export const READER_IDLE_SCENES = Object.freeze([
  quote(1, "我选择了人迹更少的一条路，而这改变了一切。", "罗伯特·弗罗斯特：《未选择的路》"),
  quote(2, "沙漠之所以美，是因为它在某处藏着一口井。", "安托万·德·圣埃克苏佩里：《小王子》"),
  quote(3, "并非所有徘徊的人都迷失了方向。", "J. R. R. 托尔金：《魔戒：护戒同盟》"),
  quote(4, "我们逆水行舟，不断被浪潮推回过去。", "F. S. 菲茨杰拉德：《了不起的盖茨比》"),
  quote(5, "从明天起，和每一个亲人通信，告诉他们我的幸福。", "海子：《面朝大海，春暖花开》"),
  quote(6, "黑夜给了我黑色的眼睛，我却用它寻找光明。", "顾城：《一代人》"),
  quote(7, "乡愁是一枚小小的邮票，我在这头，母亲在那头。", "余光中：《乡愁》"),
  quote(8, "于千万人之中，遇见你所要遇见的人。", "张爱玲：《爱》"),
  quote(9, "绝望之为虚妄，正与希望相同。", "鲁迅：《野草·希望》"),
  quote(10, "其实地上本没有路；走的人多了，也便成了路。", "鲁迅：《故乡》"),
  quote(11, "我的日子滴在时间的流里，没有声音，也没有影子。", "朱自清：《匆匆》"),
  quote(12, "这春来一天有一天的消息。", "徐志摩：《我所知道的康桥》"),
  quote(13, "轻轻的我走了，正如我轻轻的来。", "徐志摩：《再别康桥》"),
  quote(14, "撑着油纸伞，独自彷徨在悠长、悠长又寂寥的雨巷。", "戴望舒：《雨巷》"),
  quote(15, "云中谁寄锦书来？雁字回时，月满西楼。", "李清照：《一剪梅·红藕香残玉簟秋》"),
  quote(16, "欲寄彩笺兼尺素，山长水阔知何处？", "晏殊：《蝶恋花·槛菊愁烟兰泣露》"),
  quote(17, "驿寄梅花，鱼传尺素。", "秦观：《踏莎行·郴州旅舍》"),
  quote(18, "江水三千里，家书十五行。", "袁凯：《京师得家书》"),
  quote(19, "行行无别语，只道早还乡。", "袁凯：《京师得家书》"),
  quote(20, "马上相逢无纸笔，凭君传语报平安。", "岑参：《逢入京使》"),
  quote(21, "洛阳城里见秋风，欲作家书意万重。", "张籍：《秋思》"),
  quote(22, "复恐匆匆说不尽，行人临发又开封。", "张籍：《秋思》"),
  quote(23, "纵我不往，子宁不嗣音？", "佚名：《诗经·郑风·子衿》"),
  quote(24, "江南无所有，聊赠一枝春。", "陆凯：《赠范晔诗》"),
  quote(25, "海内存知己，天涯若比邻。", "王勃：《送杜少府之任蜀州》"),
  quote(26, "我寄愁心与明月，随君直到夜郎西。", "李白：《闻王昌龄左迁龙标遥有此寄》"),
  quote(27, "道路阻且长，会面安可知？", "佚名：《古诗十九首·行行重行行》"),
  quote(28, "我有所念人，隔在远远乡。", "白居易：《夜雨》"),
  quote(29, "相知无远近，万里尚为邻。", "张九龄：《送韦城李少府》"),
  quote(30, "南风知我意，吹梦到西洲。", "佚名：《西洲曲》"),
  quote(31, "折花逢驿使，寄与陇头人。", "陆凯：《赠范晔诗》"),
  quote(32, "君问归期未有期，巴山夜雨涨秋池。", "李商隐：《夜雨寄北》"),
  quote(33, "何当共剪西窗烛，却话巴山夜雨时。", "李商隐：《夜雨寄北》"),
  quote(34, "月暂晦，星常明。", "范成大：《车遥遥篇》"),
  quote(35, "一日不见，如三秋兮。", "佚名：《诗经·王风·采葛》"),
  quote(36, "故人入我梦，明我长相忆。", "杜甫：《梦李白二首·其一》"),
  quote(37, "今夜月明人尽望，不知秋思落谁家。", "王建：《十五夜望月》"),
  quote(38, "热闹是他们的，我什么也没有。", "朱自清：《荷塘月色》"),
  quote(39, "我不是归人，是个过客。", "郑愁予：《错误》"),
  quote(40, "我夜坐听风，昼眠听雨，悟得月如何缺，天如何老。", "戴望舒：《寂寞》"),
  quote(41, "笑吧，世界与你同笑；哭吧，你独自哭泣。", "艾拉·惠勒·威尔科克斯：《孤独》"),
  quote(42, "拣尽寒枝不肯栖，寂寞沙洲冷。", "苏轼：《卜算子·黄州定慧院寓居作》"),
]);

const READER_IDLE_EFFECTS = Object.freeze([
  "char-rise",
  "char-fly",
  "char-focus",
  "char-depth",
]);

const MAX_LINE_CHARACTERS = 14;
const CHARACTER_ADVANCE = 1.08;
const BALANCED_COUPLET_STAGGER = 4;

function characterLength(value) {
  return Array.from(value).length;
}

function sentenceSegments(text) {
  return text.match(/[^，；。！？]+[，；。！？]?/gu) || [text];
}

function splitLongSegment(segment) {
  const characters = Array.from(segment);
  if (characters.length <= MAX_LINE_CHARACTERS) return [segment];

  const trailingPunctuation = /[，；。！？、]/u.test(characters.at(-1))
    ? characters.pop()
    : "";
  const chunkCount = Math.ceil(
    (characters.length + (trailingPunctuation ? 1 : 0)) / MAX_LINE_CHARACTERS,
  );
  const chunkSize = Math.ceil(characters.length / chunkCount);
  const chunks = [];

  for (let index = 0; index < characters.length; index += chunkSize) {
    chunks.push(characters.slice(index, index + chunkSize).join(""));
  }
  if (trailingPunctuation) chunks[chunks.length - 1] += trailingPunctuation;
  return chunks;
}

function partitionSegments(segments, lineCount) {
  if (lineCount <= 1) return [segments.join("")];

  const totalLength = segments.reduce(
    (total, segment) => total + characterLength(segment),
    0,
  );
  const targetLength = totalLength / lineCount;
  let bestLines = null;
  let bestScore = Number.POSITIVE_INFINITY;

  const search = (startIndex, lines) => {
    const linesRemaining = lineCount - lines.length;
    if (linesRemaining === 1) {
      const candidate = [...lines, segments.slice(startIndex).join("")];
      const lengths = candidate.map(characterLength);
      const score = lengths.reduce(
        (total, length) => total + (length - targetLength) ** 2,
        0,
      );
      if (score < bestScore) {
        bestScore = score;
        bestLines = candidate;
      }
      return;
    }

    const lastEnd = segments.length - linesRemaining + 1;
    for (let endIndex = startIndex + 1; endIndex <= lastEnd; endIndex += 1) {
      search(endIndex, [...lines, segments.slice(startIndex, endIndex).join("")]);
    }
  };

  search(0, []);
  return bestLines || [segments.join("")];
}

export function layoutQuoteText(text) {
  const rawSegments = sentenceSegments(text);
  const rawLengths = rawSegments.map(characterLength);
  const isBalancedCouplet =
    rawSegments.length === 2 &&
    rawLengths[0] === rawLengths[1] &&
    rawLengths[0] <= MAX_LINE_CHARACTERS;

  if (isBalancedCouplet) {
    const lineWidths = rawLengths.map((length) => length * CHARACTER_ADVANCE);
    return {
      layout: "balanced",
      lines: rawSegments,
      starts: [0, BALANCED_COUPLET_STAGGER],
      frameUnits: Math.max(
        lineWidths[0],
        lineWidths[1] + BALANCED_COUPLET_STAGGER,
      ),
    };
  }

  const segments = rawSegments.flatMap(splitLongSegment);
  let lineCount = Math.min(3, segments.length);

  let lines = partitionSegments(segments, lineCount);
  while (
    Math.max(...lines.map(characterLength)) > MAX_LINE_CHARACTERS &&
    lineCount < Math.min(3, segments.length)
  ) {
    lineCount += 1;
    lines = partitionSegments(segments, lineCount);
  }

  const lineWidths = lines.map(
    (line) => characterLength(line) * CHARACTER_ADVANCE,
  );
  const lastLineWidth = lineWidths.at(-1);
  let frameUnits = Math.max(...lineWidths);

  // The middle line begins halfway between the first and last line starts.
  // Expand the frame only when that interpolation needs more room to keep a
  // longer middle line on one complete, uncropped row.
  for (let index = 1; index < lines.length - 1; index += 1) {
    const progress = index / (lines.length - 1);
    const minimumFrameForLine =
      (lineWidths[index] - progress * lastLineWidth) / (1 - progress);
    frameUnits = Math.max(frameUnits, minimumFrameForLine);
  }

  const lastLineStart = Math.max(0, frameUnits - lastLineWidth);
  const starts = lines.map((_line, index) => {
    if (lines.length === 1 || index === 0) return 0;
    if (index === lines.length - 1) return lastLineStart;
    return lastLineStart * (index / (lines.length - 1));
  });

  return {
    layout: lines.length === 1 ? "single" : "flow",
    lines,
    starts,
    frameUnits,
  };
}

export function chooseRandomSceneIndex(currentIndex, random = Math.random) {
  if (READER_IDLE_SCENES.length <= 1) return 0;
  const candidate = Math.floor(random() * (READER_IDLE_SCENES.length - 1));
  return candidate >= currentIndex ? candidate + 1 : candidate;
}

function motionPreference() {
  return typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-reduced-motion: reduce)").matches
    : false;
}

function pageIsVisible() {
  return !document.hidden;
}

function initialSceneIndex() {
  if (motionPreference()) return 0;
  return Math.floor(Math.random() * READER_IDLE_SCENES.length);
}

export function ReaderIdleExperience() {
  const [sceneIndex, setSceneIndex] = useState(initialSceneIndex);
  const [prefersReducedMotion, setPrefersReducedMotion] = useState(
    motionPreference,
  );
  const [isVisible, setIsVisible] = useState(pageIsVisible);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return undefined;
    const mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    const handlePreferenceChange = (event) => {
      setPrefersReducedMotion(event.matches);
      if (event.matches) setSceneIndex(0);
    };

    setPrefersReducedMotion(mediaQuery.matches);
    if (mediaQuery.matches) setSceneIndex(0);
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
    if (prefersReducedMotion || !isVisible) return undefined;

    const timer = window.setTimeout(() => {
      setSceneIndex((current) => chooseRandomSceneIndex(current));
    }, READER_IDLE_SCENE_DURATION_MS);
    return () => window.clearTimeout(timer);
  }, [isVisible, prefersReducedMotion, sceneIndex]);

  const scene = READER_IDLE_SCENES[sceneIndex];
  const effect = READER_IDLE_EFFECTS[sceneIndex % READER_IDLE_EFFECTS.length];
  const quoteLayout = layoutQuoteText(scene.text);
  const characterCount = quoteLayout.lines.reduce(
    (total, line) => total + characterLength(line),
    0,
  );
  let characterOffset = 0;

  return (
    <div
      className="reader-idle"
      data-paused={isVisible ? undefined : "true"}
      data-reduced-motion={prefersReducedMotion ? "true" : undefined}
      data-scene={sceneIndex}
    >
      <figure
        key={`${scene.id}-${effect}`}
        className="reader-idle__figure"
        data-effect={effect}
        style={{
          "--reader-idle-frame-units": quoteLayout.frameUnits.toFixed(2),
          "--source-delay": `${characterCount * 120 + 820}ms`,
        }}
        aria-hidden="true"
      >
        <blockquote
          className="reader-idle__quote"
          data-layout={quoteLayout.layout}
          data-line-count={quoteLayout.lines.length}
        >
          {quoteLayout.lines.map((line, lineIndex) => {
            const characters = Array.from(line);
            const lineStart = characterOffset;
            characterOffset += characters.length;

            return (
              <span
                className="reader-idle__line"
                key={`${scene.id}-${lineIndex}`}
                style={{
                  "--reader-idle-line-start": `${quoteLayout.starts[lineIndex]}em`,
                }}
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
        <figcaption className="reader-idle__source">
          <span className="reader-idle__source-dash" />
          <span>{scene.source}</span>
        </figcaption>
      </figure>

    </div>
  );
}
