; =============================================================
; kalkulator x86-64 wygenerowany przez Aurole
; zadanie: task_calc_add_01 (x86_64)
; operacje: dodawanie (ADD), odejmowanie (SUB), mnożenie (MUL), dzielenie (DIV), logarytm naturalny (LOG), pierwiastek kwadratowy (SQRT)
; ABI: SysV AMD64 (rdi, rsi → całkowite; xmm0 → float)
; składnia: nasm -f elf64
; =============================================================
bits 64
default rel
extern log
global calc_add, calc_sub, calc_mul, calc_div, calc_sqrt, calc_log

; --- dodawanie (ADD) ---
calc_add:
    mov RAX, RDI
    mov RBX, RSI
    add RAX, RBX
    ret

; --- odejmowanie (SUB) ---
calc_sub:
    mov RAX, RDI
    mov RBX, RSI
    sub RAX, RBX
    ret

; --- mnożenie (MUL) ---
calc_mul:
    mov RAX, RDI
    mov RBX, RSI
    imul RAX, RBX
    ret

; --- dzielenie (DIV) ---
calc_div:
    mov RAX, RDI
    mov RBX, RSI
    cqo
        idiv RBX
    ret

; --- logarytm naturalny (LOG) ---
calc_log:
    sub rsp, 8
        call log
        add rsp, 8
    ret

; --- pierwiastek kwadratowy (SQRT) ---
calc_sqrt:
    sqrtsd xmm0, xmm0
    ret

