import { Listbox, ListboxItem } from '@heroui/listbox';
import { Spinner } from '@heroui/spinner';
import { useRequest } from 'ahooks';
import { MdError } from 'react-icons/md';

import IconWrapper from '@/components/github_info/icon_wrapper';
import ItemCounter from '@/components/github_info/item_counter';
import GithubRelease from '@/components/github_info/release';
import {
  BookIcon,
  BugIcon,
  PullRequestIcon,
  StarIcon,
  TagIcon,
  UsersIcon,
  WatchersIcon,
} from '@/components/icons';

import { serverRequest } from '@/utils/request';
import { openUrl } from '@/utils/url';

import type {
  GirhubRepo,
  GithubContributor,
  GithubPullRequest,
  GithubRelease as GithubReleaseType,
} from '@/types/github';

function displayData (data: number, loading: boolean, error?: Error) {
  if (error) {
    return <MdError className='text-primary-400' />;
  }

  if (loading) {
    return <Spinner size='sm' />;
  }

  return <ItemCounter number={data} />;
}

export default function LiteyukiBotRepoInfo () {
  const repoParams = {
    owner: 'Nanaloveyuki',
    repo: 'RsLiteyukiBot',
  };

  const {
    data: snapshotData,
    error: snapshotError,
    loading: snapshotLoading,
  } = useRequest(() =>
    serverRequest.get<ServerResponse<{
      repo: GirhubRepo;
      releases: GithubReleaseType[];
      pulls: GithubPullRequest[];
      contributors: GithubContributor[];
    }>>('/base/GetGitHubRepoSnapshot', {
      params: repoParams,
    })
  );

  const repoData = snapshotData?.data?.data?.repo;
  const releases = snapshotData?.data?.data?.releases || [];
  const pulls = snapshotData?.data?.data?.pulls || [];
  const contributors = snapshotData?.data?.data?.contributors || [];
  const releaseData = releases[0];
  const prCount = pulls.length;
  const contributorsCount = contributors.length;
  const releaseCount = releases.length;

  return (
    <Listbox
      aria-label='LiteyukiBot Repo Info'
      className='p-0 gap-0 divide-y divide-default-300/50 dark:divide-default-100/80 bg-content1 max-w-[300px] overflow-visible shadow-small rounded-medium bg-opacity-50 backdrop-blur-sm'
      itemClasses={{
        base: 'px-3 first:rounded-t-medium last:rounded-b-medium rounded-none gap-3 h-12 data-[hover=true]:bg-default-100/80',
      }}
      onAction={(key: React.Key) => {
        switch (key) {
          case 'releases':
            openUrl('https://github.com/Nanaloveyuki/RsLiteyukiBot/releases', true);
            break;
          case 'contributors':
            openUrl(
              'https://github.com/Nanaloveyuki/RsLiteyukiBot/graphs/contributors',
              true
            );
            break;
          case 'license':
            openUrl(
              'https://github.com/Nanaloveyuki/RsLiteyukiBot/blob/main/LICENSE',
              true
            );
            break;
          case 'watchers':
            openUrl('https://github.com/Nanaloveyuki/RsLiteyukiBot/watchers', true);
            break;
          case 'star':
            openUrl('https://github.com/Nanaloveyuki/RsLiteyukiBot/stargazers', true);
            break;
          case 'issues':
            openUrl('https://github.com/Nanaloveyuki/RsLiteyukiBot/issues', true);
            break;
          case 'pull_requests':
            openUrl('https://github.com/Nanaloveyuki/RsLiteyukiBot/pulls', true);
            break;
          default:
            openUrl('https://github.com/Nanaloveyuki/RsLiteyukiBot', true);
        }
      }}
    >
      <ListboxItem
        key='star'
        endContent={displayData(
          repoData?.stargazers_count ?? 0,
          snapshotLoading,
          snapshotError
        )}
        startContent={
          <IconWrapper className='bg-success/10 text-success'>
            <StarIcon className='text-lg' />
          </IconWrapper>
        }
      >
        Star
      </ListboxItem>
      <ListboxItem
        key='issues'
        endContent={displayData(
          repoData?.open_issues_count ?? 0,
          snapshotLoading,
          snapshotError
        )}
        startContent={
          <IconWrapper className='bg-success/10 text-success'>
            <BugIcon className='text-lg' />
          </IconWrapper>
        }
      >
        Issues
      </ListboxItem>
      <ListboxItem
        key='pull_requests'
        endContent={displayData(prCount, snapshotLoading, snapshotError)}
        startContent={
          <IconWrapper className='bg-primary/10 text-primary'>
            <PullRequestIcon className='text-lg' />
          </IconWrapper>
        }
      >
        Pull Requests
      </ListboxItem>
      <ListboxItem
        key='releases'
        className='group h-auto py-3'
        endContent={
          snapshotError
            ? (
              <MdError className='text-primary-400' />
            )
            : snapshotLoading
              ? (
                <Spinner size='sm' />
              )
              : (
                <ItemCounter number={releaseCount} />
              )
        }
        startContent={
          <IconWrapper className='bg-primary/10 text-primary'>
            <TagIcon className='text-lg' />
          </IconWrapper>
        }
        textValue='Releases'
      >
        {releaseData && <GithubRelease releaseData={releaseData} />}
      </ListboxItem>
      <ListboxItem
        key='contributors'
        endContent={displayData(
          contributorsCount,
          snapshotLoading,
          snapshotError
        )}
        startContent={
          <IconWrapper className='bg-warning/10 text-warning'>
            <UsersIcon />
          </IconWrapper>
        }
      >
        Contributors
      </ListboxItem>
      <ListboxItem
        key='watchers'
        endContent={displayData(
          repoData?.watchers_count ?? 0,
          snapshotLoading,
          snapshotError
        )}
        startContent={
          <IconWrapper className='bg-default/50 text-foreground'>
            <WatchersIcon />
          </IconWrapper>
        }
      >
        Watchers
      </ListboxItem>
      <ListboxItem
        key='license'
        endContent={
          <span className='text-small text-default-400'>
            {repoData?.license?.name ?? 'unknown'}
          </span>
        }
        startContent={
          <IconWrapper className='bg-primary/10 text-primary dark:text-primary-500'>
            <BookIcon />
          </IconWrapper>
        }
      >
        License
      </ListboxItem>
    </Listbox>
  );
}
