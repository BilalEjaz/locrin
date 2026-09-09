import React from "react";
import { Text, type Props } from "./ui";
import { helper } from "./helper";
import "./side-effects";
import * as ns from "./ns";

export function Row(p: Props) {
  return <Text>{ns.label(helper(p))}</Text>;
}
