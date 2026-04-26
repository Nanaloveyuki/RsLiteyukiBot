import { Button } from '@heroui/button';
import { Checkbox } from '@heroui/checkbox';
import {
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
} from '@heroui/modal';
import { useEffect, useState } from 'react';
import toast from 'react-hot-toast';

export default function DesktopClosePrompt () {
  const [isOpen, setIsOpen] = useState(false);
  const [remember, setRemember] = useState(false);
  const [closeToTrayDefault, setCloseToTrayDefault] = useState(true);
  const [submitting, setSubmitting] = useState<'background' | 'exit' | null>(null);

  useEffect(() => {
    const bridge = window.__LITEYUKI_DESKTOP__;
    if (!bridge) {
      return;
    }

    return bridge.onCloseRequested((payload) => {
      setCloseToTrayDefault(payload.closeToTrayDefault);
      setRemember(false);
      setSubmitting(null);
      setIsOpen(true);
    });
  }, []);

  const closePrompt = async () => {
    setIsOpen(false);
    setSubmitting(null);
    try {
      await window.__LITEYUKI_DESKTOP__?.cancelClose();
    } catch (error) {
      console.error(error);
    }
  };

  const runAction = async (action: 'background' | 'exit') => {
    const bridge = window.__LITEYUKI_DESKTOP__;
    if (!bridge) {
      setIsOpen(false);
      return;
    }

    try {
      setSubmitting(action);
      if (action === 'background') {
        await bridge.closeToBackground(remember);
      } else {
        await bridge.exitApp(remember);
      }
      setIsOpen(false);
    } catch (error) {
      const msg = (error as Error).message;
      toast.error(`操作失败: ${msg}`);
      setSubmitting(null);
    }
  };

  return (
    <Modal
      backdrop='blur'
      isDismissable={false}
      isOpen={isOpen}
      placement='center'
      radius='lg'
      onOpenChange={(open) => {
        if (!open) void closePrompt();
      }}
    >
      <ModalContent>
        <ModalHeader className='flex flex-col gap-1'>
          关闭 Liteyuki
        </ModalHeader>
        <ModalBody className='gap-4'>
          <p className='text-sm text-default-500'>
            后端服务仍可供浏览器 Web 端访问。请选择关闭窗口后的处理方式。
          </p>
          <Checkbox
            isSelected={remember}
            onValueChange={setRemember}
          >
            始终保持此选项
          </Checkbox>
        </ModalBody>
        <ModalFooter>
          <Button
            radius='full'
            variant='flat'
            onPress={() => void closePrompt()}
          >
            取消
          </Button>
          <Button
            color={closeToTrayDefault ? 'primary' : 'default'}
            isLoading={submitting === 'background'}
            radius='full'
            variant={closeToTrayDefault ? 'solid' : 'flat'}
            onPress={() => void runAction('background')}
          >
            后台运行
          </Button>
          <Button
            color={closeToTrayDefault ? 'danger' : 'primary'}
            isLoading={submitting === 'exit'}
            radius='full'
            variant={closeToTrayDefault ? 'flat' : 'solid'}
            onPress={() => void runAction('exit')}
          >
            直接退出
          </Button>
        </ModalFooter>
      </ModalContent>
    </Modal>
  );
}
