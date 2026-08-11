import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { validateExperienceBundleV1, type ExperienceBundleV1 } from "./experience-v1.js";

describe("ExperienceBundleV1",()=>{it("round trips the shared golden fixture",()=>{
  const path=new URL("../../../spec/experience/golden/experience-bundle-v1.json",import.meta.url);
  const source=JSON.parse(readFileSync(path,"utf8")) as ExperienceBundleV1;
  validateExperienceBundleV1(source);expect(JSON.parse(JSON.stringify(source))).toEqual(source);
});});
