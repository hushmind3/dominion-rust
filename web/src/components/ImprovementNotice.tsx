import {post} from '../api'

// Manual issue creation remains available to explicit user actions.
export async function openImprovementIssue(body:{signalId:string}|{sessionId:string;description:string}){
 const issue=await post<{id:string}>('/api/rsi/issues',body)
 window.location.hash='#/rsi?issue='+encodeURIComponent(issue.id)
}

// Compatibility export for the former global self-improvement prompt.
// No polling, timer, toast, banner or automatic navigation: the user enters
// the RSI page intentionally. Failures and approvals stay in their own views.
export function ImprovementNotice(){return null}
