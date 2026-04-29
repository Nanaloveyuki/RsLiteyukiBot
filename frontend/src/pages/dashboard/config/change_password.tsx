import { Input } from '@heroui/input';
import { useLocalStorage } from '@uidotdev/usehooks';
import { Controller, useForm } from 'react-hook-form';
import toast from 'react-hot-toast';
import { useNavigate } from 'react-router-dom';

import key from '@/const/key';

import SaveButtons from '@/components/button/save_buttons';

import WebUIManager from '@/controllers/webui_manager';
import { useEffect, useState } from 'react';

const ChangePasswordCard = () => {
  const [authState, setAuthState] = useState<WebUiAuthState | null>(null);
  const {
    control,
    handleSubmit: handleWebuiSubmit,
    formState: { isSubmitting, errors },
    reset,
    watch,
  } = useForm<{
    oldPassword: string;
    newPassword: string;
  }>({
    defaultValues: {
      oldPassword: '',
      newPassword: '',
    },
  });

  const navigate = useNavigate();
  const [, setToken] = useLocalStorage(key.token, '');

  // 监听旧密码的值
  const oldPasswordValue = watch('oldPassword');

  useEffect(() => {
    WebUIManager.getAuthState()
      .then(setAuthState)
      .catch(() => undefined);
  }, []);

  const onSubmit = handleWebuiSubmit(async (data) => {
    try {
      await WebUIManager.changePassword(data.oldPassword, data.newPassword);

      toast.success(authState?.passwordConfigured ? '修改成功' : '设置成功');
      setToken('');
      localStorage.removeItem(key.token);
      navigate('/web_login');
    } catch (error) {
      const msg = (error as Error).message;
      toast.error(`修改失败: ${msg}`);
    }
  });

  return (
    <>
      <title>修改密码 - Liteyuki WebUI</title>

      <Controller
        control={control}
        name='oldPassword'
        rules={{
          validate: (value) => {
            if (authState?.passwordConfigured && (!value || value.trim().length === 0)) {
              return '旧密码不能为空';
            }
            return true;
          },
        }}
        render={({ field }) => (
          <Input
            {...field}
            label={authState?.passwordConfigured ? '旧密码' : '首次定密无需旧密码'}
            placeholder={authState?.passwordConfigured ? '请输入旧密码' : '首次设置密码时可留空'}
            type='password'
            isRequired={!!authState?.passwordConfigured}
            isInvalid={!!errors.oldPassword}
            errorMessage={errors.oldPassword?.message}
          />
        )}
      />

      <Controller
        control={control}
        name='newPassword'
        rules={{
          required: '新密码不能为空',
          minLength: {
            value: 6,
            message: '新密码至少需要6个字符',
          },
          validate: (value) => {
            if (!value || value.trim().length === 0) {
              return '新密码不能为空';
            }
            if (value.trim().length !== value.length) {
              return '新密码不能包含前后空格';
            }
            if (authState?.passwordConfigured && value === oldPasswordValue) {
              return '新密码不能与旧密码相同';
            }
            if (!/[a-zA-Z]/.test(value)) {
              return '新密码必须包含字母';
            }
            if (!/[0-9]/.test(value)) {
              return '新密码必须包含数字';
            }
            return true;
          },
        }}
        render={({ field }) => (
          <Input
            {...field}
            label='新密码'
            placeholder='至少6位，包含字母和数字'
            type='password'
            isRequired
            isInvalid={!!errors.newPassword}
            errorMessage={errors.newPassword?.message}
          />
        )}
      />

      <SaveButtons
        onSubmit={onSubmit}
        reset={reset}
        isSubmitting={isSubmitting}
      />
    </>
  );
};

export default ChangePasswordCard;
