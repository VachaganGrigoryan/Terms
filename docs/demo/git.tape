Output ../img/git.gif
Set Height 640
Source common.tape

Hide
Type "cd /demo/repo && clear"
Enter
Show

Type "tg status"
Sleep 400ms
Enter
Sleep 3s
Type "clear"
Enter
Type "tg diff"
Sleep 400ms
Enter
Sleep 3s
Type "clear"
Enter
Type "tg stage README.md"
Sleep 400ms
Enter
Sleep 2500ms
Type "tg stage --apply README.md"
Sleep 400ms
Enter
Sleep 2s
Type "clear"
Enter
Type "tg commit -m 'Update README'"
Sleep 400ms
Enter
Sleep 5s
Type "clear"
Enter
Type "tg commit --apply -m 'Update README' | tail -4"
Sleep 400ms
Enter
Sleep 3s
Type "tg status"
Sleep 400ms
Enter
Sleep 3s
