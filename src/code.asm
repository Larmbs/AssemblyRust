a dd, 0
b dd, 1
c dd, 0
mov CX, 10
step PROC
mov EAX, [a] ; take A
   add EAX, [b] ; add B
   mov [c], EAX ; move result to C
   
   mov EAX, [b] ; 
   mov [a], EAX ; move B to A

   mov EAX, [c] ;
   mov [b], EAX ; move C to B
   ret
step END

loop:
   call step
   print [c]
   dec CX
   jnz loop