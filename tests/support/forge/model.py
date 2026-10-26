"""Provider wire shapes derived from provider.rs and sync/repository.rs."""
import subprocess


def allocate(state):
    value = state['next_id']
    state['next_id'] += 1
    return value


def create(state, kind, fields, provider='gh'):
    branch = fields.get('head', fields.get('source_branch', ''))
    existing = next((p for p in state[kind].values()
                     if kind == 'pulls' and branch and p['branch'] == branch), None)
    counter = 'next_number' if provider == 'gh' else 'next_' + kind + '_number'
    number = state.get(counter, 1)
    if existing is None:
        occupied = set(state[kind])
        if provider == 'gh':
            occupied |= set(state['issues']) | set(state['pulls'])
        while str(number) in occupied:
            number += 1
        state[counter] = number + 1
    else:
        number = existing['number']
    if branch and branch not in state['branches']:
        # Production creates and pushes a real local branch before publication.
        head = subprocess.run(['git', 'rev-parse', '--verify', '--end-of-options', branch],
                              capture_output=True, text=True, check=False)
        if head.returncode == 0:
            state['branches'][branch] = head.stdout.strip()
    item = dict(kind=kind, number=number, id=existing['id'] if existing else allocate(state), title=fields.get('title', ''),
                body=fields.get('body', fields.get('description', '')), state='open',
                assignees=[], labels=fields.get('labels', []), comments=[], reviews=[],
                review_comments=[], discussions=[], versions=[], links=[], children=[], timeline=[],
                branch=branch, base=fields.get('base', fields.get('target_branch', 'main')),
                sha=state['branches'].get(branch, ''), draft=fields.get('draft', False),
                updated_at=state['now'], merged_at=None, merge_sha=None)
    if isinstance(item['labels'], str):
        item['labels'] = [label for label in item['labels'].split(',') if label]
    item['draft'] |= item['title'].startswith(('Draft: ', 'WIP: '))
    if existing is not None:
        # Publication fills in seeded metadata without discarding scenario history.
        existing.update({key: item[key] for key in
                         ['title', 'body', 'branch', 'base', 'sha', 'draft', 'updated_at']})
        existing['labels'] = list(dict.fromkeys(existing['labels'] + item['labels']))
        item = existing
    state[kind][str(number)] = item
    return item


def checks(state, item):
    return state['checks'].get(item['sha'], [])


def github(state, item, graphql=False):
    number = item['number']
    url = f"https://github.com/{state['repo']}/{'pull' if item['kind'] == 'pulls' else 'issues'}/{number}"
    labels = [dict(name=name) for name in item['labels']]
    if graphql:
        return dict(number=number, title=item['title'], body=item['body'], url=url,
                    headRefName=item['branch'], baseRefName=item['base'], headRefOid=item['sha'],
                    state='MERGED' if item['merged_at'] else item['state'].upper(),
                    isDraft=item['draft'], mergedAt=item['merged_at'], updatedAt=item['updated_at'],
                    mergeCommit=dict(oid=item['merge_sha']) if item['merge_sha'] else None,
                    mergeStateStatus=item.get('merge_status', 'CLEAN'), labels=labels,
                    statusCheckRollup=[dict(status=c['status'].upper(),
                        conclusion=c['conclusion'].upper() if c.get('conclusion') else None)
                        for c in checks(state, item)])
    value = dict(id=item['id'], number=number, title=item['title'], body=item['body'],
                 html_url=url, state=item['state'], draft=item['draft'], labels=labels,
                 head=dict(ref=item['branch'], sha=item['sha']), base=dict(ref=item['base']),
                 assignees=[dict(login=login) for login in item['assignees']],
                 user=dict(login=state['login'], type='User'), updated_at=item['updated_at'],
                 merged_at=item['merged_at'], closed_at=item['merged_at'])
    if item['kind'] == 'pulls':
        value['pull_request'] = dict(url=url)
    return value


def gitlab(state, item):
    pipelines = state['pipelines'].get(item['sha'], [])
    return dict(id=item['id'], iid=item['number'], title=item['title'], description=item['body'],
                web_url=f"https://gitlab.com/group/repo/-/{'merge_requests' if item['kind'] == 'pulls' else 'issues'}/{item['number']}",
                state='merged' if item['merged_at'] else ('opened' if item['state'] == 'open' else 'closed'),
                draft=item['draft'], work_in_progress=item['draft'], sha=item['sha'],
                source_branch=item['branch'], target_branch=item['base'], labels=item['labels'],
                assignees=[dict(id=1, username=login) for login in item['assignees']],
                author=dict(id=1, username=state['login']), updated_at=item['updated_at'],
                merged_at=item['merged_at'], merge_commit_sha=item['merge_sha'],
                detailed_merge_status=item.get('merge_status', 'mergeable'),
                diff_refs=dict(head_sha=item['sha'], base_sha=state['branches'].get(item['base'], ''),
                               start_sha=state['branches'].get(item['base'], '')),
                head_pipeline=pipelines[0] if pipelines else None)


def comment(state, fields):
    return dict(id=allocate(state), body=fields['body'], user=dict(login=state['login']),
                author=dict(id=1, username=state['login']), created_at=state['now'],
                updated_at=state['now'], system=False, type=None, resolvable=False, resolved=False)


def merge(state, item, sha, delete):
    if sha and sha != item['sha']:
        raise ValueError('head SHA mismatch')
    if item['draft'] or item['state'] != 'open':
        raise ValueError('pull request must be open and ready')
    item.update(state='closed', merged_at=state['now'], merge_sha=f"{allocate(state):040x}",
                merge_method='squash')
    state['branches'][item['base']] = item['merge_sha']
    if delete:
        state['branches'].pop(item['branch'], None)
