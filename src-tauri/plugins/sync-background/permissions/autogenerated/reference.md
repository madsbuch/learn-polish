## Default Permission

Scheduling the background sync, and the job that finishes downloading courses kept offline. The app's Rust code calls it; the web view never needs to.

#### This default permission set includes the following:

- `allow-schedule`
- `allow-cancel`
- `allow-schedule-offline`

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`sync-background:allow-cancel`

</td>
<td>

Enables the cancel command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`sync-background:deny-cancel`

</td>
<td>

Denies the cancel command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`sync-background:allow-schedule`

</td>
<td>

Enables the schedule command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`sync-background:deny-schedule`

</td>
<td>

Denies the schedule command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`sync-background:allow-schedule-offline`

</td>
<td>

Enables the schedule_offline command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`sync-background:deny-schedule-offline`

</td>
<td>

Denies the schedule_offline command without any pre-configured scope.

</td>
</tr>
</table>
