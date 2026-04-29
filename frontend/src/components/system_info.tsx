import { Button } from '@heroui/button';
import { Card, CardBody, CardHeader } from '@heroui/card';
import { Chip } from '@heroui/chip';
import { Input } from '@heroui/input';
import { Pagination } from '@heroui/pagination';
import { Select, SelectItem } from '@heroui/select';
import { Spinner } from '@heroui/spinner';
import { Tabs, Tab } from '@heroui/tabs';
import { Tooltip } from '@heroui/tooltip';
import { useLocalStorage, useDebounce } from '@uidotdev/usehooks';
import { useRequest } from 'ahooks';
import clsx from 'clsx';
import { useEffect, useState } from 'react';
import { FaCircleInfo } from 'react-icons/fa6';
import { IoDownloadOutline, IoLogoChrome, IoLogoOctocat, IoSearch } from 'react-icons/io5';
import { IoMdCheckmark, IoMdFlash, IoMdSettings } from 'react-icons/io';
import { RiMacFill } from 'react-icons/ri';

import key from '@/const/key';
import MirrorManager from '@/controllers/mirror_manager';
import WebUIManager, {
  type LatestReleaseResponse,
  type ReleaseAssetInfo,
  type ReleaseVersionInfo,
} from '@/controllers/webui_manager';
import Modal from '@/components/modal';
import MirrorSelectorModal from '@/components/mirror_selector_modal';
import { openUrl } from '@/utils/url';
import { compareVersion, hasNewVersion } from '@/utils/version';

export interface SystemInfoItemProps {
  title: string;
  icon?: React.ReactNode;
  value?: React.ReactNode;
  endContent?: React.ReactNode;
  hasBackground?: boolean;
  onClick?: () => void;
  clickable?: boolean;
}

const SystemInfoItem: React.FC<SystemInfoItemProps> = ({
  title,
  value = '--',
  icon,
  endContent,
  hasBackground = false,
  onClick,
  clickable = false,
}) => {
  return (
    <div
      className={clsx(
        'flex items-baseline gap-3 py-2 text-sm transition-colors',
        hasBackground ? 'text-white/90' : 'text-default-600 dark:text-gray-300',
        clickable && 'cursor-pointer rounded-lg -mx-2 px-2 hover:bg-default-100/50 dark:hover:bg-default-800/30'
      )}
      onClick={onClick}
    >
      <div className='self-center text-lg opacity-70'>{icon}</div>
      <div className='w-24 font-medium'>{title}</div>
      <div className={clsx(
        'flex-1 text-xs font-mono',
        hasBackground ? 'text-white/80' : 'text-default-500'
      )}
      >
        {value}
      </div>
      <div className='self-center'>{endContent}</div>
    </div>
  );
};

function formatFileSize(size: number | undefined) {
  if (!size) {
    return '--';
  }

  if (size >= 1024 * 1024 * 1024) {
    return `${(size / 1024 / 1024 / 1024).toFixed(1)} GB`;
  }
  if (size >= 1024 * 1024) {
    return `${(size / 1024 / 1024).toFixed(1)} MB`;
  }
  if (size >= 1024) {
    return `${(size / 1024).toFixed(1)} KB`;
  }
  return `${size} B`;
}

function formatReleaseTime(timestamp?: string) {
  if (!timestamp) {
    return '--';
  }

  return new Date(timestamp).toLocaleString();
}

function releaseDisplayName(release: ReleaseVersionInfo) {
  return release.name?.trim() || release.tag;
}

function resolveDownloadUrl(asset?: ReleaseAssetInfo) {
  if (!asset) {
    return null;
  }
  return asset.mirrorDownloadUrl || asset.downloadUrl;
}

function platformDownloadHint(latestRelease: LatestReleaseResponse) {
  return `${latestRelease.platform.os} / ${latestRelease.platform.arch}`;
}

function getMirrorDisplayName(mirror?: string) {
  if (!mirror) {
    return 'GitHub 原始';
  }

  try {
    return new URL(mirror).hostname;
  } catch {
    return mirror;
  }
}

function formatLatency(latency: number) {
  if (latency >= 5000) return '>5s';
  if (latency >= 1000) return `${(latency / 1000).toFixed(1)}s`;
  return `${latency}ms`;
}

function getLatencyColor(latency: number | null): 'success' | 'warning' | 'danger' | 'default' {
  if (latency === null) return 'default';
  if (latency < 300) return 'success';
  if (latency < 1000) return 'warning';
  return 'danger';
}

const ReleaseActions: React.FC<{
  release: ReleaseVersionInfo;
  title?: string;
}> = ({ release, title = '适配当前平台的下载资产' }) => {
  const asset = release.recommendedAsset;
  const primaryDownloadUrl = resolveDownloadUrl(asset);

  return (
    <Card className='border border-default-200/70 bg-default-50/70 shadow-none dark:border-default-100/10 dark:bg-default-100/5'>
      <CardBody className='gap-4 p-4'>
        <div className='space-y-1'>
          <div className='text-sm font-medium text-default-800 dark:text-default-100'>{title}</div>
          {asset
            ? (
              <>
                <div className='flex flex-wrap items-center gap-2 text-sm'>
                  <Chip size='sm' color='primary' variant='flat'>{asset.name}</Chip>
                  <span className='text-default-500'>{formatFileSize(asset.size)}</span>
                </div>
                <div className='text-xs text-default-500'>
                  上传时间: {formatReleaseTime(asset.updatedAt)}
                </div>
              </>
            )
            : (
              <div className='text-sm text-warning-600 dark:text-warning-400'>
                当前平台没有命中的发布资产，请直接打开发布页手动选择。
              </div>
            )}
        </div>

        <div className='flex flex-wrap gap-2'>
          <Button
            color='primary'
            startContent={<IoDownloadOutline size={16} />}
            onPress={() => {
              if (primaryDownloadUrl) {
                openUrl(primaryDownloadUrl, true);
              } else {
                openUrl(release.mirrorHtmlUrl || release.htmlUrl, true);
              }
            }}
          >
            {asset ? '下载推荐资产' : '打开发布页'}
          </Button>
          {asset?.mirrorDownloadUrl && asset.mirrorDownloadUrl !== asset.downloadUrl && (
            <Button
              variant='flat'
              onPress={() => openUrl(asset.downloadUrl, true)}
            >
              GitHub 原始下载
            </Button>
          )}
          {asset?.mirrorDownloadUrl && asset.mirrorDownloadUrl !== asset.downloadUrl && (
            <Button
              variant='flat'
              onPress={() => openUrl(asset.mirrorDownloadUrl!, true)}
            >
              镜像下载
            </Button>
          )}
          <Button
            variant='light'
            onPress={() => openUrl(release.mirrorHtmlUrl || release.htmlUrl, true)}
          >
            打开发布页
          </Button>
        </div>
      </CardBody>
    </Card>
  );
};

const LatestReleaseDialogContent: React.FC<{
  currentVersion: string;
  latestRelease: LatestReleaseResponse;
}> = ({ currentVersion, latestRelease }) => {
  return (
    <div className='space-y-4'>
      <div className='flex flex-col gap-4 rounded-xl border border-default-200/70 bg-default-50/70 px-5 py-6 dark:border-default-100/10 dark:bg-default-100/5'>
        <div className='flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between'>
          <div className='space-y-1'>
            <div className='text-xs font-medium uppercase tracking-wider text-default-500'>当前版本</div>
            <Chip size='md' variant='flat' color='default'>v{currentVersion}</Chip>
          </div>
          <div className='space-y-1'>
            <div className='text-xs font-medium uppercase tracking-wider text-primary-500'>最新版本</div>
            <Chip size='md' variant='shadow' color='primary'>{latestRelease.latest.tag}</Chip>
          </div>
        </div>
        <div className='text-xs text-default-500'>
          推荐平台: {platformDownloadHint(latestRelease)}
        </div>
      </div>

      <ReleaseActions release={latestRelease.latest} />
    </div>
  );
};

interface VersionSelectDialogProps {
  currentVersion: string;
  onClose: () => void;
}

const VersionSelectDialogContent: React.FC<VersionSelectDialogProps> = ({
  currentVersion,
  onClose,
}) => {
  const [selectedVersion, setSelectedVersion] = useState<ReleaseVersionInfo | null>(null);
  const [currentPage, setCurrentPage] = useState(1);
  const [releaseFilter, setReleaseFilter] = useState<'release' | 'all'>('release');
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedMirror, setSelectedMirror] = useState<string | undefined>(undefined);
  const [mirrorLatency, setMirrorLatency] = useState<number | null>(null);
  const [mirrorTesting, setMirrorTesting] = useState(false);
  const [mirrorModalOpen, setMirrorModalOpen] = useState(false);
  const debouncedSearch = useDebounce(searchQuery, 300);

  const pageSize = 15;
  const {
    data: releasesData,
    loading: releasesLoading,
    error: releasesError,
  } = useRequest(
    () => WebUIManager.getAllReleases({
      page: currentPage,
      pageSize,
      type: releaseFilter,
      search: debouncedSearch,
      mirror: selectedMirror,
    }),
    {
      refreshDeps: [currentPage, releaseFilter, debouncedSearch, selectedMirror],
    }
  );

  const versions = releasesData?.versions || [];
  const pagination = releasesData?.pagination;

  useEffect(() => {
    if (versions.length === 0) {
      setSelectedVersion(null);
      return;
    }

    const stillExists = selectedVersion
      ? versions.find((item) => item.tag === selectedVersion.tag)
      : null;
    setSelectedVersion(stillExists || versions[0] || null);
  }, [versions, selectedVersion]);

  const testCurrentMirror = async () => {
    setMirrorTesting(true);
    try {
      const result = await MirrorManager.testSingleMirror(selectedMirror || '', 'file');
      setMirrorLatency(result.success ? result.latency : null);
    } catch {
      setMirrorLatency(null);
    } finally {
      setMirrorTesting(false);
    }
  };

  const isOlderRelease = selectedVersion
    ? compareVersion(selectedVersion.tag, currentVersion) < 0
    : false;

  return (
    <div className='space-y-4'>
      <div className='flex flex-col gap-3 rounded-xl border border-default-200/70 bg-default-50/70 p-4 dark:border-default-100/10 dark:bg-default-100/5 sm:flex-row sm:items-center sm:justify-between'>
        <div className='flex items-center gap-2'>
          <span className='text-sm text-default-600'>当前版本:</span>
          <Chip color='primary' variant='flat' size='sm'>
            v{currentVersion}
          </Chip>
        </div>
        <div className='text-xs text-default-500'>
          当前平台: {releasesData?.platform.displayName || '--'}
        </div>
      </div>

      <Tabs
        selectedKey={releaseFilter}
        onSelectionChange={(key) => {
          setReleaseFilter(key as 'release' | 'all');
          setCurrentPage(1);
          setSearchQuery('');
        }}
        size='sm'
        color='primary'
        variant='underlined'
        classNames={{ tabList: 'gap-4' }}
      >
        <Tab key='release' title='正式版本' />
        <Tab key='all' title='全部版本' />
      </Tabs>

      <Card className='bg-default-100/50 shadow-sm'>
        <CardBody className='px-3 py-2'>
          <div className='flex items-center justify-between gap-3'>
            <div className='flex flex-wrap items-center gap-2'>
              <span className='text-xs text-default-500'>镜像源:</span>
              <span className='text-sm font-medium'>{getMirrorDisplayName(selectedMirror || releasesData?.mirror)}</span>
              {mirrorLatency !== null && (
                <Chip
                  size='sm'
                  color={getLatencyColor(mirrorLatency)}
                  variant='flat'
                  startContent={<IoMdCheckmark size={12} />}
                >
                  {formatLatency(mirrorLatency)}
                </Chip>
              )}
              {mirrorLatency === null && !mirrorTesting && (
                <Chip size='sm' color='default' variant='flat'>未测试</Chip>
              )}
              {mirrorTesting && (
                <Chip size='sm' color='primary' variant='flat'>测速中...</Chip>
              )}
            </div>
            <div className='flex items-center gap-1'>
              <Tooltip content='测速'>
                <Button
                  isIconOnly
                  size='sm'
                  variant='light'
                  isLoading={mirrorTesting}
                  onPress={testCurrentMirror}
                >
                  <IoMdFlash size={16} />
                </Button>
              </Tooltip>
              <Tooltip content='切换镜像'>
                <Button
                  isIconOnly
                  size='sm'
                  variant='light'
                  onPress={() => setMirrorModalOpen(true)}
                >
                  <IoMdSettings size={16} />
                </Button>
              </Tooltip>
            </div>
          </div>
        </CardBody>
      </Card>

      <Input
        placeholder='搜索版本号...'
        size='sm'
        value={searchQuery}
        onValueChange={(value) => {
          setSearchQuery(value);
          setCurrentPage(1);
        }}
        startContent={<IoSearch className='text-default-400' />}
        isClearable
        onClear={() => setSearchQuery('')}
        classNames={{ inputWrapper: 'h-9' }}
      />

      <div className='space-y-2'>
        <div className='flex items-center justify-between'>
          <label className='text-sm font-medium text-default-700'>选择版本</label>
          {pagination && (
            <span className='text-xs text-default-400'>共 {pagination.total} 个版本</span>
          )}
        </div>
        {releasesLoading
          ? (
            <div className='flex items-center gap-2 py-2'>
              <Spinner size='sm' />
              <span className='text-sm text-default-500'>加载版本列表...</span>
            </div>
          )
          : releasesError
            ? (
              <div className='text-sm text-danger-500'>
                加载版本列表失败: {releasesError.message}
              </div>
            )
            : versions.length === 0
              ? (
                <div className='py-4 text-center text-sm text-default-500'>
                  {searchQuery ? `未找到匹配 "${searchQuery}" 的版本` : '暂无可用版本'}
                </div>
              )
              : (
                <Select
                  label='选择版本'
                  placeholder='请选择要下载的版本'
                  selectedKeys={selectedVersion ? [selectedVersion.tag] : []}
                  onSelectionChange={(keys) => {
                    const selectedTag = Array.from(keys)[0] as string;
                    const version = versions.find((item) => item.tag === selectedTag);
                    setSelectedVersion(version || null);
                  }}
                  classNames={{ trigger: 'h-auto min-h-10' }}
                >
                  {versions.map((version) => {
                    const isCurrent = version.tag.replace(/^v/i, '') === currentVersion;
                    const isDowngrade = compareVersion(version.tag, currentVersion) < 0;

                    return (
                      <SelectItem key={version.tag} textValue={version.tag}>
                        <div className='flex flex-col gap-1'>
                          <div className='flex items-center gap-2'>
                            <span className='max-w-[320px] truncate'>{releaseDisplayName(version)}</span>
                            {version.type === 'prerelease' && (
                              <Chip size='sm' color='secondary' variant='flat'>预发布</Chip>
                            )}
                            {isCurrent && (
                              <Chip size='sm' color='success' variant='flat'>当前</Chip>
                            )}
                            {isDowngrade && !isCurrent && (
                              <Chip size='sm' color='warning' variant='flat'>旧版本</Chip>
                            )}
                          </div>
                          <div className='flex flex-wrap items-center gap-2 text-xs text-default-400'>
                            <span className='font-mono'>{version.tag}</span>
                            <span>{formatReleaseTime(version.publishedAt || version.createdAt)}</span>
                            <span>{version.assets.length} 个资产</span>
                          </div>
                        </div>
                      </SelectItem>
                    );
                  })}
                </Select>
              )}
      </div>

      {selectedVersion && (
        <div className='space-y-3'>
          <div className='flex flex-wrap items-center gap-2'>
            <Chip size='sm' color='primary' variant='flat'>{selectedVersion.tag}</Chip>
            {selectedVersion.type === 'prerelease' && (
              <Chip size='sm' color='secondary' variant='flat'>预发布</Chip>
            )}
            {isOlderRelease && (
              <Chip size='sm' color='warning' variant='flat'>低于当前版本</Chip>
            )}
          </div>

          {selectedVersion.body && (
            <div className='rounded-lg border border-default-200/70 bg-default-50/70 p-3 text-sm text-default-600 dark:border-default-100/10 dark:bg-default-100/5 dark:text-default-300'>
              {selectedVersion.body}
            </div>
          )}

          {isOlderRelease && (
            <div className='rounded-lg border border-warning-200/60 bg-warning-50/70 p-3 text-xs text-warning-700 dark:border-warning-700/40 dark:bg-warning-900/20 dark:text-warning-300'>
              这是一个低于当前运行版本的历史 release。Liteyuki 不会执行本地自更新；如需回退，请自行下载对应资产并替换安装。
            </div>
          )}

          <ReleaseActions release={selectedVersion} />
        </div>
      )}

      {pagination && pagination.totalPages > 1 && (
        <div className='flex justify-center'>
          <Pagination
            total={pagination.totalPages}
            page={currentPage}
            onChange={setCurrentPage}
            size='sm'
            showControls
          />
        </div>
      )}

      <div className='flex justify-end border-t border-default-100 pt-4 dark:border-default-100/10'>
        <Button variant='flat' onPress={onClose}>关闭</Button>
      </div>

      <MirrorSelectorModal
        isOpen={mirrorModalOpen}
        onClose={() => setMirrorModalOpen(false)}
        currentMirror={selectedMirror}
        onSelect={(mirror) => {
          setSelectedMirror(mirror || undefined);
          setMirrorLatency(null);
        }}
        type='file'
      />
    </div>
  );
};

interface AppVersionProps {
  hasBackground?: boolean;
}

const AppVersion: React.FC<AppVersionProps> = ({ hasBackground = false }) => {
  const [isVersionModalOpen, setIsVersionModalOpen] = useState(false);
  const [isLatestModalOpen, setIsLatestModalOpen] = useState(false);
  const {
    data: packageData,
    loading: packageLoading,
    error: packageError,
  } = useRequest(WebUIManager.getAppVersion, {
    cacheKey: 'app-version',
    staleTime: 60 * 60 * 1000,
    cacheTime: 24 * 60 * 60 * 1000,
  });
  const {
    data: latestReleaseData,
    loading: latestReleaseLoading,
  } = useRequest(() => WebUIManager.getLatestRelease(), {
    cacheKey: 'liteyuki-latest-release',
    staleTime: 10 * 60 * 1000,
    cacheTime: 30 * 60 * 1000,
  });

  const currentVersion = packageData?.version || '';
  const hasNewRelease = !!(
    currentVersion
    && latestReleaseData?.latest?.tag
    && hasNewVersion(currentVersion, latestReleaseData.latest.tag)
  );

  return (
    <>
      <SystemInfoItem
        title='Liteyuki 版本'
        icon={<IoLogoOctocat className='text-xl' />}
        hasBackground={hasBackground}
        value={
          packageError
            ? `错误：${packageError.message}`
            : packageLoading
              ? <Spinner size='sm' />
              : (
                <Tooltip content='点击查看版本与下载'>
                  <span
                    className='cursor-pointer underline decoration-dashed underline-offset-2 transition-colors hover:text-primary-500'
                    onClick={() => setIsVersionModalOpen(true)}
                  >
                    {currentVersion}
                  </span>
                </Tooltip>
              )
        }
        endContent={
          hasNewRelease
            ? (
              <Tooltip content='发现新版本'>
                <div className='flex cursor-pointer items-center justify-center' onClick={() => setIsLatestModalOpen(true)}>
                  <Chip
                    size='sm'
                    color='primary'
                    variant='flat'
                    classNames={{
                      content: 'flex items-center justify-center px-1 text-[10px] font-bold',
                      base: 'h-5 min-h-5 min-w-[42px]',
                    }}
                  >
                    {latestReleaseLoading ? <Spinner size='sm' color='primary' classNames={{ wrapper: 'h-3 w-3' }} /> : 'New'}
                  </Chip>
                </div>
              </Tooltip>
            )
            : null
        }
      />

      {isVersionModalOpen && (
        <Modal
          title='版本管理'
          size='lg'
          hideFooter
          onClose={() => setIsVersionModalOpen(false)}
          content={
            <VersionSelectDialogContent
              currentVersion={currentVersion}
              onClose={() => setIsVersionModalOpen(false)}
            />
          }
        />
      )}

      {isLatestModalOpen && latestReleaseData && hasNewRelease && (
        <Modal
          title='发现新版本'
          size='lg'
          hideFooter
          onClose={() => setIsLatestModalOpen(false)}
          content={
            <LatestReleaseDialogContent
              currentVersion={currentVersion}
              latestRelease={latestReleaseData}
            />
          }
        />
      )}
    </>
  );
};

export interface SystemInfoProps {
  archInfo?: string;
}

const SystemInfo: React.FC<SystemInfoProps> = ({ archInfo }) => {
  const [backgroundImage] = useLocalStorage<string>(key.backgroundImage, '');
  const hasBackground = !!backgroundImage;

  return (
    <Card className={clsx(
      'flex-1 overflow-visible border border-white/40 shadow-sm backdrop-blur-sm dark:border-white/10',
      hasBackground ? 'bg-white/10 dark:bg-black/10' : 'bg-white/60 dark:bg-black/40'
    )}
    >
      <CardHeader className={clsx(
        'items-center gap-2 px-4 pt-4 pb-0 font-bold',
        hasBackground ? 'text-white drop-shadow-sm' : 'text-default-700 dark:text-white'
      )}
      >
        <FaCircleInfo className='text-lg opacity-80' />
        <span>系统信息</span>
      </CardHeader>
      <CardBody className='flex-1'>
        <div className='flex h-full flex-col justify-between gap-2'>
          <AppVersion hasBackground={hasBackground} />
          <SystemInfoItem
            title='WebUI 版本'
            icon={<IoLogoChrome className='text-xl' />}
            value='Next'
            hasBackground={hasBackground}
          />
          <SystemInfoItem
            title='系统版本'
            icon={<RiMacFill className='text-xl' />}
            value={archInfo}
            hasBackground={hasBackground}
          />
        </div>
      </CardBody>
    </Card>
  );
};

export default SystemInfo;
