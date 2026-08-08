import { expect, it } from "vitest";
import {
  buildOptimizationAnnotations,
  optimizationAnnotationText,
  projectOptimizationAnnotations,
} from "./ComposeOptimizationReviewDialog.jsx";

it("marks deleted original text and inserted optimized text independently", () => {
  const annotations = buildOptimizationAnnotations("您好，朋友", "您好，老朋友");

  expect(optimizationAnnotationText(annotations.left)).toBe("您好，朋友");
  expect(optimizationAnnotationText(annotations.right)).toBe("您好，老朋友");
  expect(
    annotations.left.filter(({ changed }) => changed).map(({ character }) => character),
  ).toEqual([]);
  expect(
    annotations.right.filter(({ changed }) => changed).map(({ character }) => character),
  ).toEqual(["老"]);
});

it("keeps inherited difference marks but leaves newly edited text unmarked", () => {
  const { right } = buildOptimizationAnnotations("项目顺利", "项目进展顺利");
  const edited = projectOptimizationAnnotations(right, "项目近期进展顺利");
  const newlyAdded = edited.slice(2, 4);
  const inheritedDifference = edited.slice(4, 6);

  expect(newlyAdded.map(({ character }) => character).join("")).toBe("近期");
  expect(newlyAdded.every(({ changed }) => !changed)).toBe(true);
  expect(inheritedDifference.map(({ character }) => character).join("")).toBe("进展");
  expect(inheritedDifference.every(({ changed }) => changed)).toBe(true);
});

it("handles long bodies without allocating an unbounded comparison matrix", () => {
  const original = `${"甲".repeat(1300)}旧${"乙".repeat(1300)}`;
  const optimized = `${"甲".repeat(1300)}新${"乙".repeat(1300)}`;
  const annotations = buildOptimizationAnnotations(original, optimized);

  expect(optimizationAnnotationText(annotations.left)).toBe(original);
  expect(optimizationAnnotationText(annotations.right)).toBe(optimized);
  expect(annotations.left.filter(({ changed }) => changed)).toHaveLength(1);
  expect(annotations.right.filter(({ changed }) => changed)).toHaveLength(1);
});
