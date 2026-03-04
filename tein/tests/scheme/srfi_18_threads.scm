;;; srfi/18 — threads, mutexes, condition variables
;;; chibi threads are cooperative green threads

(import (srfi 18))

;; --- threads ---

;; thread? predicate
(test-true "thread/main-is-thread?" (thread? (current-thread)))

;; create and start a simple thread
(test-equal "thread/join-result" 42
  (let ((t (make-thread (lambda () 42))))
    (thread-start! t)
    (thread-join! t)))

;; thread state: unstarted thread doesn't run
(test-true "thread/unstarted-ok"
  (let ((ran #f))
    (make-thread (lambda () (set! ran #t)))
    (not ran)))

;; thread-yield is safe
(test-true "thread/yield-ok"
  (begin (thread-yield!) #t))

;; thread-sleep! (very short)
(test-true "thread/sleep-ok"
  (begin (thread-sleep! 0.001) #t))

;; join with timeout on infinite loop → returns timeout value
(test-equal "thread/join-timeout" 'timed-out
  (let ((t (make-thread (lambda () (let lp () (lp))))))
    (thread-start! t)
    (thread-join! t 0.05 'timed-out)))

;; --- mutexes ---

(test-true  "mutex/is-mutex?" (mutex? (make-mutex)))
(test-equal "mutex/lock-unlock" 'done
  (let ((m (make-mutex)))
    (mutex-lock! m)
    (mutex-unlock! m)
    'done))

;; mutex exclusive access between threads: both items logged
(test-equal "mutex/exclusive" 2
  (let ((m   (make-mutex))
        (log '()))
    (define (with-lock thunk)
      (mutex-lock! m)
      (let ((r (thunk)))
        (mutex-unlock! m)
        r))
    (let ((t (make-thread
               (lambda ()
                 (with-lock (lambda () (set! log (cons 1 log))))))))
      (thread-start! t)
      (with-lock (lambda () (set! log (cons 2 log))))
      (thread-join! t)
      (length log))))  ; both threads logged → 2 items

;; --- condition variables ---

(test-true "condvar/is-condvar?" (condition-variable? (make-condition-variable)))

;; signal wakes a waiting thread
(test-equal "condvar/signal-wait" 'signalled
  (let ((m  (make-mutex))
        (cv (make-condition-variable))
        (result #f))
    (mutex-lock! m)
    (let ((t (make-thread
               (lambda ()
                 (mutex-lock! m)
                 (set! result 'signalled)
                 (mutex-unlock! m)
                 (condition-variable-signal! cv)))))
      (thread-start! t)
      (mutex-unlock! m cv 0.5)   ; unlock + wait with timeout
      (thread-join! t)
      result)))
