import {post} from '../api'

// Explicit owner action only. No background RSI polling, toast or notification.
export async function openImprovementIssue(body:{signalId:string}|{sessionId:string;description:string}){
 const issue=await post<{id:string}>('/api/rsi/issues',body)
 window.location.hash='#/rsi?issue='+encodeURIComponent(issue.id)
}
