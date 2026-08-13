import { Modal, type ModalProps } from "antd";
import { X } from "@phosphor-icons/react";

type AppModalProps = Omit<ModalProps, "title"> & {
  title: string;
};

export function AppModal({ className, width = 620, title, ...props }: AppModalProps) {
  return (
    <Modal
      {...props}
      centered
      closeIcon={<X size={18} />}
      destroyOnHidden
      width={width}
      title={title}
      className={["app-modal", className].filter(Boolean).join(" ")}
    />
  );
}
