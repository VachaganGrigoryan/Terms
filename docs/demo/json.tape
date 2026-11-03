Output ../img/json.gif
Set Height 520
Source common.tape

Hide
Type "cd /demo && clear"
Enter
Show

Type "tq -c 'map(.fish)' data.json"
Sleep 400ms
Enter
Sleep 2s
Type `tq '.[] | select(.fish=="tuna")' data.json`
Sleep 400ms
Enter
Sleep 3s
Type "clear"
Enter
Type "tq '{orders: length}' data.json"
Sleep 400ms
Enter
Sleep 2500ms
