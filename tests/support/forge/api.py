"""The gh/glab operations used by provider, dispatch, sync and claim code."""
from urllib.parse import unquote
from model import allocate, create, github, gitlab, comment, merge


def cli(state, provider, args, options):
    if provider == 'glab':
        if args == ['mr', 'list']:
            return collection(state, 'pulls', 'GET', {'state': 'opened'}, gitlab)
        raise ValueError(f'unsupported command {args}')
    family, action = args[:2]
    if len(args) != (2 if action in {'create', 'list'} else 3):
        raise ValueError(f'unsupported command {args}')
    if family == 'pr' and action == 'create':
        fields = {key.removeprefix('--'): value for key, value in options.items()}
        item = create(state, 'pulls', fields)
        return github(state, item)['html_url']
    if family == 'pr' and action == 'list':
        return [github(state, p, True) for p in state['pulls'].values()
                if options.get('--state', 'open') == 'all'
                or ('merged' if p['merged_at'] else p['state']) == options.get('--state', 'open')
                ][:int(options.get('--limit', 30))]
    number = args[2].rstrip('/').split('/')[-1]
    if family == 'issue' and action == 'close':
        state['issues'][number]['state'] = 'closed'
        return ''
    if family != 'pr':
        raise ValueError(f'unsupported command {args}')
    item = state['pulls'].get(number)
    if item is None:
        item = next((p for p in state['pulls'].values() if p['branch'] == args[2]), None)
    if item is None:
        raise ValueError('pull request not found')
    if action == 'view':
        return github(state, item, True)
    if action == 'ready':
        item['draft'] = False
        item['title'] = item['title'].removeprefix('Draft: ').removeprefix('WIP: ')
    elif action == 'merge':
        if not options.get('--squash'):
            raise ValueError('only squash merge is supported')
        merge(state, item, options.get('--match-head-commit'), options.get('--delete-branch'))
    elif action == 'review':
        review = comment(state, dict(body=options.get('--body', '')))
        review.update(state='CHANGES_REQUESTED' if options.get('--request-changes') else
                      'APPROVED' if options.get('--approve') else 'COMMENTED',
                      commit_id=item['sha'], submitted_at=state['now'])
        item['reviews'].append(review)
    else:
        raise ValueError(f'unsupported command {args}')
    return ''


def collection(state, kind, method, fields, wire):
    if method == 'POST':
        return wire(state, create(state, kind, fields, 'gh' if wire == github else 'glab'))
    if method != 'GET':
        raise ValueError(f'unsupported collection method {method}')
    items = list(state[kind].values())
    if kind == 'issues' and wire == github:
        items += list(state['pulls'].values())
    wanted = fields.get('state', 'open' if wire == github else 'all')
    items = [p for p in items if wanted == 'all' or
             ('merged' if wire == gitlab and p['merged_at'] else p['state'])
             == wanted.replace('opened', 'open')]
    branch = fields.get('source_branch', fields.get('head', '').split(':', 1)[-1])
    if branch:
        items = [p for p in items if p['branch'] == branch]
    return [wire(state, item) for item in items]


def notes(state, rows, tail, method, fields):
    if len(tail) > 1:
        raise ValueError('unsupported comment endpoint')
    if not tail:
        if method == 'GET':
            return rows
        if method == 'POST':
            note = comment(state, fields)
            rows.append(note)
            return note
    else:
        note = next(n for n in rows if str(n['id']) == tail[0])
        if method == 'GET':
            return note
        if method in {'PATCH', 'PUT'}:
            note.update(body=fields['body'], updated_at=state['now'])
            return note
        if method == 'DELETE':
            rows.remove(note)
            return None
    raise ValueError('unsupported comment command')


def edit(state, item, fields):
    for key in ['title', 'body', 'description', 'state']:
        if key in fields:
            item['body' if key == 'description' else key] = fields[key]
    if 'title' in fields:
        item['draft'] = item['title'].startswith(('Draft: ', 'WIP: '))
    if 'state_event' in fields:
        item['state'] = {'close': 'closed', 'reopen': 'open'}[fields['state_event']]
    if 'assignee_ids' in fields:
        ids = fields['assignee_ids']
        item['assignees'] = [state['login']] if ids and ids != '0' else []
    if 'labels' in fields:
        item['labels'] = fields['labels'].split(',') if isinstance(fields['labels'], str) else fields['labels']
    for label in fields.get('remove_labels', '').split(','):
        if label in item['labels']:
            item['labels'].remove(label)
    for label in fields.get('add_labels', '').split(','):
        if label and label not in item['labels']:
            item['labels'].append(label)
    item['updated_at'] = state['now']


def api(state, provider, path, method, fields):
    if path == 'user' and method == 'GET':
        return dict(id=1, login=state['login'], username=state['login'])
    if path == 'search/issues' and provider == 'gh' and method == 'GET':
        return dict(incomplete_results=False, items=[github(state, p) for p in state['pulls'].values()
                                                   if p['merged_at']])
    prefix = f"repos/{state['repo']}/" if provider == 'gh' else f"projects/{state['project']}/"
    if not path.startswith(prefix):
        raise ValueError(f'unsupported endpoint {path}')
    parts = path[len(prefix):].split('/')
    wire = github if provider == 'gh' else gitlab
    if parts == ['labels']:
        if method == 'GET':
            return [dict(name=name) for name in state['labels']]
    if parts[0] == 'commits' and len(parts) == 3 and parts[2] == 'check-runs' and method == 'GET':
        rows = state['checks'].get(parts[1], [])
        size = int(fields.get('per_page', 30))
        start = (int(fields.get('page', 1)) - 1) * size
        return dict(total_count=len(rows), check_runs=rows[start:start + size])
    if parts[0] in {'git', 'repository'}:
        return branches(state, parts, method, fields)
    if parts[:2] == ['issues', 'comments']:
        for item in list(state['issues'].values()) + list(state['pulls'].values()):
            if any(str(n['id']) == parts[2] for n in item['comments']):
                return notes(state, item['comments'], parts[2:], method, fields)
        raise ValueError('comment not found')
    if parts[0] not in ({'issues', 'pulls'} if provider == 'gh' else {'issues', 'merge_requests'}):
        raise ValueError(f'unsupported endpoint {path}')
    kind = 'issues' if parts[0] == 'issues' else 'pulls'
    if len(parts) == 1:
        return collection(state, kind, method, fields, wire)
    number = parts[1]
    item = state[kind].get(number)
    if item is None and kind == 'issues' and provider == 'gh':
        item = state['pulls'].get(number)
    if item is None:
        raise ValueError(f'item {number} not found')
    if len(parts) == 2:
        if method in {'PATCH', 'PUT'}:
            edit(state, item, fields)
        elif method != 'GET':
            raise ValueError(f'unsupported item method {method}')
        if item.get('auto_merge') and not item['merged_at'] and pipeline_passed(state, item):
            merge(state, item, item['sha'], item['remove_source_branch'])
        return wire(state, item)
    endpoint, tail = parts[2], parts[3:]
    if tail and endpoint not in {'comments', 'notes', 'labels', 'versions', 'discussions'}:
        raise ValueError(f'unsupported endpoint {path}')
    if endpoint in {'labels', 'versions'} and len(tail) > 1:
        raise ValueError(f'unsupported endpoint {path}')
    if endpoint in {'comments', 'notes'}:
        rows = item['review_comments'] if parts[0] == 'pulls' else item['comments']
        return notes(state, rows, tail, method, fields)
    if endpoint == 'assignees':
        logins = fields.get('assignees', [])
        if method == 'POST':
            item['assignees'] = list(dict.fromkeys(item['assignees'] + logins))
        elif method == 'DELETE':
            item['assignees'] = [a for a in item['assignees'] if a not in logins]
        elif method != 'GET':
            raise ValueError('unsupported assignee method')
        return wire(state, item)
    if endpoint == 'labels':
        if method == 'DELETE' and tail:
            item['labels'].remove(unquote(tail[0]))
        elif method == 'POST':
            item['labels'] = list(dict.fromkeys(item['labels'] + fields['labels']))
        elif method != 'GET':
            raise ValueError('unsupported label method')
        return [dict(name=name) for name in item['labels']]
    if endpoint == 'timeline' and method == 'GET':
        return item['timeline']
    if endpoint == 'reviews':
        if method == 'GET':
            return item['reviews']
        if method == 'POST':
            review = comment(state, fields)
            review.update(state={'APPROVE': 'APPROVED', 'REQUEST_CHANGES': 'CHANGES_REQUESTED',
                                 'COMMENT': 'COMMENTED'}[fields['event']],
                          commit_id=fields.get('commit_id', item['sha']), submitted_at=state['now'])
            item['reviews'].append(review)
            return review
    if endpoint == 'merge' and method == 'PUT':
        if str(fields.get('squash', '')).lower() != 'true':
            raise ValueError('only squash merge is supported')
        if fields.get('sha') and fields['sha'] != item['sha']:
            raise ValueError('head SHA mismatch')
        if item['draft'] or item['state'] != 'open':
            raise ValueError('pull request must be open and ready')
        if str(fields.get('auto_merge', '')).lower() == 'true':
            item['auto_merge'] = True
            item['remove_source_branch'] = fields.get('should_remove_source_branch') in {'true', True}
            if not pipeline_passed(state, item):
                return wire(state, item)
        merge(state, item, fields.get('sha'), fields.get('should_remove_source_branch') in {'true', True})
        return wire(state, item)
    if endpoint == 'pipelines' and method == 'GET':
        # Tests can include old-SHA pipelines to exercise head selection.
        return state['pipelines'].get(item['sha'], [])
    if endpoint in {'sub_issues', 'links'}:
        rows = item['children' if endpoint == 'sub_issues' else 'links']
        if method == 'GET':
            return rows
        if method == 'POST':
            if endpoint == 'sub_issues':
                child = next(i for i in state['issues'].values() if i['id'] == fields['sub_issue_id'])
                rows.append(github(state, child))
                return rows[-1]
            target = state['issues'][str(fields['target_issue_iid'])]
            link = dict(id=allocate(state), link_type=fields['link_type'],
                        source_issue=dict(iid=item['number']), target_issue=dict(iid=target['number']),
                        target_project_id=fields['target_project_id'])
            rows.append(link)
            target['links'].append(link | dict(link_type='is_blocked_by'))
            return link
    if endpoint == 'versions' and method == 'GET':
        return next(v for v in item['versions'] if str(v['id']) == tail[0]) if tail else item['versions']
    if endpoint == 'discussions':
        return discussions(state, item, tail, method, fields)
    raise ValueError(f'unsupported {method} {path}')


def discussions(state, item, tail, method, fields):
    if not tail:
        if method == 'GET':
            return item['discussions']
        if method == 'POST':
            note = comment(state, fields)
            note.update(type='DiffNote' if fields.get('position') else 'DiscussionNote',
                        resolvable=True, position=fields.get('position'))
            discussion = dict(id=str(allocate(state)), individual_note=False, notes=[note])
            item['discussions'].append(discussion)
            return discussion
    else:
        discussion = next(d for d in item['discussions'] if d['id'] == tail[0])
        if len(tail) > 1 and tail[1] == 'notes':
            note = notes(state, discussion['notes'], tail[2:], method, fields)
            if method == 'POST':
                note.update(type='DiscussionNote', resolvable=True,
                            resolved=discussion['notes'][0]['resolved'])
            return note
        if len(tail) != 1:
            raise ValueError('unsupported discussion endpoint')
        if method == 'PUT':
            for note in discussion['notes']:
                note['resolved'] = fields['resolved']
        elif method != 'GET':
            raise ValueError('unsupported discussion method')
        return discussion
    raise ValueError('unsupported discussion command')


def branches(state, parts, method, fields):
    if parts in [['git', 'refs'], ['repository', 'branches']] and method == 'POST':
        name = fields.get('branch', fields.get('ref', '').removeprefix('refs/heads/'))
        sha = fields.get('sha', state['branches'].get(fields.get('ref'), fields.get('ref')))
        state['branches'][name] = sha
        return dict(ref='refs/heads/' + name, name=name, object=dict(sha=sha), commit=dict(id=sha))
    if not ((parts[:3] in [['git', 'ref', 'heads'], ['git', 'refs', 'heads']] and len(parts) > 3)
            or (parts[:2] == ['repository', 'branches'] and len(parts) > 2)):
        raise ValueError('unsupported branch endpoint')
    name = unquote('/'.join(parts[3:] if parts[0] == 'git' else parts[2:]))
    if method == 'DELETE':
        del state['branches'][name]
        return None
    if method == 'GET':
        sha = state['branches'][name]
        return dict(ref='refs/heads/' + name, name=name, object=dict(sha=sha), commit=dict(id=sha))
    raise ValueError('unsupported branch command')


def pipeline_passed(state, item):
    return any(p.get('sha') == item['sha'] and p.get('status') == 'success'
               for p in state['pipelines'].get(item['sha'], [])[:1])
