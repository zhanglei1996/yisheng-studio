import type { Icon as PhosphorIcon } from "@phosphor-icons/react";
import type { ComponentProps } from "react";

export function antdIcon(Icon: PhosphorIcon, size = 16) {
  return function AntdPhosphorIcon(props: ComponentProps<"span">) {
    return <span {...props}><Icon size={size} /></span>;
  };
}
