~~~
$quill: meeting_minutes@1.0
$kind: main
organization: Example Software Foundation
body_name: Board of Directors
meeting_type: regular
date: 2026-09-15
called_to_order: 2026-09-15T18:02
location: Video call
chair: Ada Lovelace
secretary: Grace Hopper
quorum: 4
attendees:
  - name: Ada Lovelace
    role: Chair
  - name: Grace Hopper
    role: Secretary
  - name: Alan Turing
    role: Treasurer
  - name: Katherine Johnson
  - name: Edsger Dijkstra
    present: false
  - name: Margaret Hamilton
    role: Executive Director
    voting: false
adjourned: 2026-09-15T19:31
next_meeting: 2026-10-20
~~~

The chair called the meeting to order and noted that the agenda had been circulated
a week in advance. The minutes of the **August 18** meeting were approved as circulated.

~~~
$kind: agenda_item
topic: Treasurer's report
presenter: Alan Turing
action_items:
  - owner: Alan Turing
    task: Circulate the Q3 statement to the board
    due: 2026-09-30
~~~

Income for the quarter was ahead of plan, driven by two new *corporate sponsors*.
Expenses tracked budget except for conference travel.

~~~
$kind: agenda_item
topic: Adopt the revised trademark policy
presenter: Margaret Hamilton
outcome:
  value: motion
  motion_text: Adopt the trademark policy **as circulated** on September 8.
  moved_by: Katherine Johnson
  seconded_by: Alan Turing
  votes_for: 3
  votes_against: 0
  abstentions: 1
  result: carried
action_items:
  - owner: Margaret Hamilton
    task: Publish the policy on the website
    due: 2026-09-22
  - owner: Grace Hopper
    task: Notify downstream distributors
~~~

Discussion centred on how the policy treats community events. The executive director
confirmed that non-commercial meetups need no prior approval.

- Fork naming guidance was clarified.
- Logo usage examples will be added later.
